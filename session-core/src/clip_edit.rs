// Clip (transport block) editing: move and trim the draggable play segments on
// the timeline by rewriting the event stream. A faithful port of the frontend's
// clipEditOps.ts so the editor runs the SAME logic via WASM.
//
// A TransportBlock is one draggable unit: a regular play segment, or a whole run
// of loop iterations (which always moves as a unit). Mixer/automation events are
// deliberately untouched by moves/trims: automation stays at wall time.

use crate::event::SessionEvent;
use crate::timeline::{Clip, LoopRegion};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

pub const MIN_BLOCK_MS: f64 = 100.0;
const EPS_MS: f64 = 1.0;

// Every event type that changes deck position or play state.
const TRANSPORT_TYPES: &[&str] = &[
    "deck_snapshot",
    "load_track",
    "eject_track",
    "play",
    "stop",
    "stopped_at_cue",
    "stop_at_cue",
    "cue_set_and_stop",
    "cue_preview_start",
    "cue_preview_end",
    "cue_move",
    "seek",
    "loop_out",
    "loop_in",
    "exit_loop",
    "reloop",
];

fn is_transport(event_type: &str) -> bool {
    TRANSPORT_TYPES.contains(&event_type)
}

fn near(first: f64, second: f64) -> bool {
    (first - second).abs() <= EPS_MS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportBlock {
    pub deck: String,
    pub block_id: u32,
    pub start_ms: f64,
    pub end_ms: f64,
    pub track_path: String,
    pub track_start_sec: f64,
    pub playback_rate: f64,
    #[serde(rename = "loop")]
    pub loop_region: Option<LoopRegion>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveResult {
    pub events: Vec<SessionEvent>,
    pub applied_delta_ms: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimResult {
    pub events: Vec<SessionEvent>,
    pub applied_ms: f64,
}

pub fn blocks_for_deck(clips: &[Clip], deck: &str) -> Vec<TransportBlock> {
    let mut order: Vec<u32> = Vec::new();
    let mut groups: HashMap<u32, Vec<&Clip>> = HashMap::new();
    for clip in clips {
        if clip.deck != deck {
            continue;
        }
        let group = groups.entry(clip.block_id).or_default();
        if group.is_empty() {
            order.push(clip.block_id);
        }
        group.push(clip);
    }

    let mut blocks: Vec<TransportBlock> = Vec::new();
    for id in &order {
        let group = &groups[id];
        let first = group[0];
        let start_ms = group
            .iter()
            .map(|c| c.session_start_ms)
            .fold(f64::INFINITY, f64::min);
        let end_ms = group
            .iter()
            .map(|c| c.session_end_ms)
            .fold(f64::NEG_INFINITY, f64::max);
        blocks.push(TransportBlock {
            deck: deck.to_string(),
            block_id: first.block_id,
            start_ms,
            end_ms,
            track_path: first.track_path.clone(),
            track_start_sec: first.track_start_sec,
            playback_rate: first.playback_rate,
            loop_region: first.loop_region.clone(),
        });
    }
    blocks.sort_by(|a, b| {
        a.start_ms
            .partial_cmp(&b.start_ms)
            .unwrap_or(Ordering::Equal)
    });
    blocks
}

// Events that start a block from silence at an explicit position.
fn start_events_for(block: &TransportBlock, at_ms: f64) -> Vec<SessionEvent> {
    if let Some(region) = &block.loop_region {
        // track_start_sec is the wrapped entry position, which may sit inside the
        // region; playing from the loop start instead would shift the block.
        vec![
            SessionEvent {
                sec: Some(block.track_start_sec),
                ..SessionEvent::at(at_ms, "play", &block.deck)
            },
            SessionEvent {
                start_sec: Some(region.start_sec),
                end_sec: Some(region.end_sec),
                ..SessionEvent::at(at_ms, "loop_out", &block.deck)
            },
        ]
    } else {
        vec![SessionEvent {
            sec: Some(block.track_start_sec),
            ..SessionEvent::at(at_ms, "play", &block.deck)
        }]
    }
}

fn end_events_for(block: &TransportBlock, at_ms: f64) -> Vec<SessionEvent> {
    if block.loop_region.is_some() {
        // exit_loop, not a bare stop: a glued loop block must be disarmed, else
        // the relocated clip would wrap at the stale loop boundary.
        vec![
            SessionEvent::at(at_ms, "exit_loop", &block.deck),
            SessionEvent::at(at_ms, "stop", &block.deck),
        ]
    } else {
        vec![SessionEvent::at(at_ms, "stop", &block.deck)]
    }
}

fn stable_sort_by_ms(mut events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    events.sort_by(crate::sim::event_sim_order);
    events
}

struct Neighborhood {
    prev: Option<TransportBlock>,
    next: Option<TransportBlock>,
    min_start_ms: f64,
    max_end_ms: f64,
    own_load_ms: Option<f64>,
}

fn neighborhood_of(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
) -> Option<Neighborhood> {
    let blocks = blocks_for_deck(clips, &block.deck);
    let index = blocks
        .iter()
        .position(|c| near(c.start_ms, block.start_ms) && near(c.end_ms, block.end_ms))?;
    let prev = if index > 0 {
        Some(blocks[index - 1].clone())
    } else {
        None
    };
    let next = if index < blocks.len() - 1 {
        Some(blocks[index + 1].clone())
    } else {
        None
    };

    let deck = block.deck.as_str();
    let mut own_load_ms: Option<f64> = None;
    for event in events {
        if event.deck.as_deref() != Some(deck) || event.event_type != "load_track" {
            continue;
        }
        if event.elapsed_ms <= block.start_ms + EPS_MS {
            own_load_ms = Some(match own_load_ms {
                None => event.elapsed_ms,
                Some(existing) => existing.max(event.elapsed_ms),
            });
        }
    }

    let mut min_start_ms = prev.as_ref().map(|p| p.end_ms).unwrap_or(0.0).max(0.0);
    let mut max_end_ms = next.as_ref().map(|n| n.start_ms).unwrap_or(f64::INFINITY);
    for event in events {
        if event.deck.as_deref() != Some(deck) {
            continue;
        }
        let is_load = event.event_type == "load_track";
        let is_left_barrier = event.event_type == "eject_track"
            || (event.event_type == "deck_snapshot" && event.path.is_some());
        if !is_load && !is_left_barrier {
            continue;
        }
        if event.elapsed_ms <= block.start_ms + EPS_MS {
            if is_left_barrier {
                min_start_ms = min_start_ms.max(event.elapsed_ms);
            }
        } else if event.elapsed_ms >= block.end_ms - EPS_MS {
            max_end_ms = max_end_ms.min(event.elapsed_ms);
        }
    }

    Some(Neighborhood {
        prev,
        next,
        min_start_ms,
        max_end_ms,
        own_load_ms,
    })
}

// Signed audio seconds the deck advances between two session times, following
// the piecewise-constant rate curve and nudges.
fn audio_seconds_between(
    events: &[SessionEvent],
    deck: &str,
    from_ms: f64,
    to_ms: f64,
    fallback_rate: f64,
) -> f64 {
    if from_ms == to_ms {
        return 0.0;
    }
    let sign = if to_ms >= from_ms { 1.0 } else { -1.0 };
    let lower = from_ms.min(to_ms);
    let upper = from_ms.max(to_ms);

    let mut rate = fallback_rate;
    let mut nudge = 1.0;
    let mut total = 0.0;
    let mut cursor = lower;
    for event in events {
        if event.deck.as_deref() != Some(deck) {
            continue;
        }
        if event.elapsed_ms >= upper {
            break;
        }
        let is_rate = event.event_type == "set_playback_rate" && event.rate.is_some();
        let is_snapshot = event.event_type == "deck_snapshot" && event.playback_rate.is_some();
        let is_nudge = event.event_type == "set_nudge" && event.percent.is_some();
        if !is_rate && !is_snapshot && !is_nudge {
            continue;
        }
        if event.elapsed_ms > lower {
            total += ((event.elapsed_ms - cursor) / 1000.0) * rate * nudge;
            cursor = event.elapsed_ms;
        }
        if is_rate {
            rate = event.rate.unwrap();
        }
        if is_snapshot {
            rate = event.playback_rate.unwrap();
        }
        if is_nudge {
            nudge = 1.0 + event.percent.unwrap() / 100.0;
        }
    }
    total += ((upper - cursor) / 1000.0) * rate * nudge;
    sign * total
}

// A resume-play (no sec) takes its position from the previous block's end. When
// boundary events are rewritten, that implicit dependency must become explicit.
fn normalize_resume_play(event: &SessionEvent, next: Option<&TransportBlock>) -> SessionEvent {
    if let Some(next) = next {
        if event.event_type == "play"
            && event.sec.is_none()
            && near(event.elapsed_ms, next.start_ms)
        {
            let mut out = event.clone();
            out.sec = Some(next.track_start_sec);
            return out;
        }
    }
    event.clone()
}

// A transport event sitting exactly on the block's start boundary. Returns the
// replacement, or None when the event is consumed (synthesized start takes over).
fn rewrite_start_boundary(event: &SessionEvent) -> Option<SessionEvent> {
    if event.event_type == "deck_snapshot" {
        // The snapshot also loads the track and seeds rate/cue state, so it must
        // survive; only its transport effects move with the block.
        let mut out = event.clone();
        out.is_playing = Some(false);
        out.loop_active = Some(false);
        Some(out)
    } else {
        None
    }
}

pub fn block_bounds(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
) -> Option<(f64, f64)> {
    neighborhood_of(events, clips, block).map(|n| (n.min_start_ms, n.max_end_ms))
}

// An UNPLAYED track loaded inside the span the moved block now occupies is
// destroyed: its load_track and the setup config (rate, beat grid) it emitted,
// up to the next load, are dropped. Returns indices into `events`.
fn orphaned_load_events(
    events: &[SessionEvent],
    deck: &str,
    own_load_ms: Option<f64>,
    span_start_ms: f64,
    span_end_ms: f64,
) -> HashSet<usize> {
    let mut discarded: HashSet<usize> = HashSet::new();
    let mut load_ms: Vec<f64> = events
        .iter()
        .filter(|e| e.deck.as_deref() == Some(deck) && e.event_type == "load_track")
        .map(|e| e.elapsed_ms)
        .collect();
    load_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let play_ms: Vec<f64> = events
        .iter()
        .filter(|e| e.deck.as_deref() == Some(deck) && e.event_type == "play")
        .map(|e| e.elapsed_ms)
        .collect();

    for (idx, load) in events.iter().enumerate() {
        if load.deck.as_deref() != Some(deck) || load.event_type != "load_track" {
            continue;
        }
        if let Some(own) = own_load_ms {
            if near(load.elapsed_ms, own) {
                continue;
            }
        }
        if load.elapsed_ms < span_start_ms - EPS_MS || load.elapsed_ms > span_end_ms + EPS_MS {
            continue;
        }
        let next_load_ms = load_ms
            .iter()
            .copied()
            .find(|&ms| ms > load.elapsed_ms + EPS_MS)
            .unwrap_or(f64::INFINITY);
        let was_played = play_ms
            .iter()
            .any(|&ms| ms > load.elapsed_ms + EPS_MS && ms < next_load_ms - EPS_MS);
        if was_played {
            continue;
        }
        discarded.insert(idx);
        for (j, event) in events.iter().enumerate() {
            if event.deck.as_deref() != Some(deck) {
                continue;
            }
            if event.event_type != "set_playback_rate" && event.event_type != "set_beat_grid" {
                continue;
            }
            if event.elapsed_ms >= load.elapsed_ms - EPS_MS
                && event.elapsed_ms < next_load_ms - EPS_MS
            {
                discarded.insert(j);
            }
        }
    }
    discarded
}

pub fn move_transport_block(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
    delta_ms: f64,
) -> MoveResult {
    let Some(neighborhood) = neighborhood_of(events, clips, block) else {
        return MoveResult {
            events: events.to_vec(),
            applied_delta_ms: 0.0,
        };
    };
    let Neighborhood {
        prev,
        next,
        min_start_ms,
        max_end_ms,
        own_load_ms,
    } = neighborhood;
    let t0 = block.start_ms;
    let t1 = block.end_ms;

    let applied = (min_start_ms - t0).max((max_end_ms - t1).min(delta_ms));
    if applied.abs() < 1.0 {
        return MoveResult {
            events: events.to_vec(),
            applied_delta_ms: 0.0,
        };
    }

    let new_start = t0 + applied;
    let new_end = t1 + applied;
    let new_load_ms = own_load_ms.map(|own| own.min(new_start));
    let load_shift = match (new_load_ms, own_load_ms) {
        (Some(nl), Some(own)) => nl - own,
        _ => 0.0,
    };

    let deck = block.deck.as_str();
    let in_load_window = |event: &SessionEvent| -> bool {
        let Some(own) = own_load_ms else {
            return false;
        };
        if event.deck.as_deref() != Some(deck) {
            return false;
        }
        if event.event_type == "load_track" {
            return near(event.elapsed_ms, own);
        }
        if event.event_type == "set_playback_rate" || event.event_type == "set_beat_grid" {
            return event.elapsed_ms >= own - EPS_MS && event.elapsed_ms < t0 - EPS_MS;
        }
        false
    };

    let discarded = orphaned_load_events(events, deck, own_load_ms, new_start, new_end);
    let prev_glued = prev.as_ref().is_some_and(|p| near(p.end_ms, t0));
    let next_glued = next.as_ref().is_some_and(|n| near(n.start_ms, t1));

    let mut kept: Vec<SessionEvent> = Vec::new();
    for (idx, event) in events.iter().enumerate() {
        if discarded.contains(&idx) {
            continue;
        }
        if in_load_window(event) {
            let mut out = event.clone();
            out.elapsed_ms = (event.elapsed_ms + load_shift).min(new_start);
            kept.push(out);
            continue;
        }
        if event.deck.as_deref() != Some(deck) || !is_transport(&event.event_type) {
            kept.push(event.clone());
            continue;
        }
        let ms = event.elapsed_ms;
        if ms < t0 - EPS_MS || ms > t1 + EPS_MS {
            kept.push(normalize_resume_play(event, next.as_ref()));
            continue;
        }
        if event.event_type == "load_track" || event.event_type == "eject_track" {
            kept.push(event.clone());
            continue;
        }
        if near(ms, t0) {
            if let Some(replacement) = rewrite_start_boundary(event) {
                kept.push(replacement);
            }
            continue;
        }
        if near(ms, t1) {
            continue;
        }
        let mut out = event.clone();
        out.elapsed_ms = ms + applied;
        kept.push(out);
    }

    if prev_glued {
        if let Some(prev) = &prev {
            kept.extend(end_events_for(prev, t0));
        }
    }
    if next_glued {
        if let Some(next) = &next {
            kept.extend(start_events_for(next, t1));
        }
    }
    kept.extend(start_events_for(block, new_start));
    kept.extend(end_events_for(block, new_end));

    MoveResult {
        events: stable_sort_by_ms(kept),
        applied_delta_ms: applied,
    }
}

pub fn trim_transport_block(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
    edge: Edge,
    new_ms: f64,
) -> TrimResult {
    let unchanged_ms = if edge == Edge::Start {
        block.start_ms
    } else {
        block.end_ms
    };
    // Loop blocks move as one unit; trimming them is not supported.
    if block.loop_region.is_some() {
        return TrimResult {
            events: events.to_vec(),
            applied_ms: unchanged_ms,
        };
    }
    let Some(neighborhood) = neighborhood_of(events, clips, block) else {
        return TrimResult {
            events: events.to_vec(),
            applied_ms: unchanged_ms,
        };
    };
    let Neighborhood {
        prev,
        next,
        min_start_ms,
        max_end_ms,
        ..
    } = neighborhood;
    let t0 = block.start_ms;
    let t1 = block.end_ms;
    let deck = block.deck.as_str();

    if edge == Edge::Start {
        let earliest_by_audio = t0 - (block.track_start_sec / block.playback_rate) * 1000.0;
        let lower = min_start_ms.max(earliest_by_audio);
        let applied = lower.max((t1 - MIN_BLOCK_MS).min(new_ms));
        if near(applied, t0) {
            return TrimResult {
                events: events.to_vec(),
                applied_ms: t0,
            };
        }
        let new_sec = (block.track_start_sec
            + audio_seconds_between(events, deck, t0, applied, block.playback_rate))
        .max(0.0);

        let prev_glued = prev.as_ref().is_some_and(|p| near(p.end_ms, t0));
        let mut kept: Vec<SessionEvent> = Vec::new();
        for event in events {
            if event.deck.as_deref() != Some(deck) || !is_transport(&event.event_type) {
                kept.push(event.clone());
                continue;
            }
            if near(event.elapsed_ms, t0) && event.event_type != "load_track" {
                if let Some(replacement) = rewrite_start_boundary(event) {
                    kept.push(replacement);
                }
                continue;
            }
            kept.push(event.clone());
        }
        kept.push(SessionEvent {
            sec: Some(new_sec),
            ..SessionEvent::at(applied, "play", deck)
        });
        if prev_glued {
            if let Some(prev) = &prev {
                kept.extend(end_events_for(prev, t0));
            }
        }
        return TrimResult {
            events: stable_sort_by_ms(kept),
            applied_ms: applied,
        };
    }

    let applied = (t0 + MIN_BLOCK_MS).max(max_end_ms.min(new_ms));
    if near(applied, t1) {
        return TrimResult {
            events: events.to_vec(),
            applied_ms: t1,
        };
    }
    let next_glued = next.as_ref().is_some_and(|n| near(n.start_ms, t1));

    let mut kept: Vec<SessionEvent> = Vec::new();
    for event in events {
        if event.deck.as_deref() != Some(deck) || !is_transport(&event.event_type) {
            kept.push(event.clone());
            continue;
        }
        if near(event.elapsed_ms, t1)
            && event.event_type != "load_track"
            && event.event_type != "eject_track"
        {
            continue;
        }
        kept.push(normalize_resume_play(event, next.as_ref()));
    }
    kept.push(SessionEvent::at(applied, "stop", deck));
    if next_glued {
        if let Some(next) = &next {
            kept.extend(start_events_for(next, t1));
        }
    }
    TrimResult {
        events: stable_sort_by_ms(kept),
        applied_ms: applied,
    }
}

// The load_track (and the rate/grid it seeded before play) for the block being
// deleted, but only when no OTHER play uses that load, so a shared track stays.
fn orphaned_own_load(
    events: &[SessionEvent],
    deck: &str,
    own_load_ms: Option<f64>,
    block_t0: f64,
    block_t1: f64,
) -> HashSet<usize> {
    let mut discarded: HashSet<usize> = HashSet::new();
    let Some(own) = own_load_ms else {
        return discarded;
    };
    let next_load = events
        .iter()
        .filter(|e| {
            e.deck.as_deref() == Some(deck)
                && e.event_type == "load_track"
                && e.elapsed_ms > own + EPS_MS
        })
        .map(|e| e.elapsed_ms)
        .fold(f64::INFINITY, f64::min);
    let shared = events.iter().any(|e| {
        e.deck.as_deref() == Some(deck)
            && e.event_type == "play"
            && e.elapsed_ms > own + EPS_MS
            && e.elapsed_ms < next_load - EPS_MS
            && (e.elapsed_ms < block_t0 - EPS_MS || e.elapsed_ms > block_t1 + EPS_MS)
    });
    if shared {
        return discarded;
    }
    for (idx, event) in events.iter().enumerate() {
        if event.deck.as_deref() != Some(deck) {
            continue;
        }
        if (event.event_type == "load_track" && near(event.elapsed_ms, own))
            || ((event.event_type == "set_playback_rate" || event.event_type == "set_beat_grid")
                && event.elapsed_ms >= own - EPS_MS
                && event.elapsed_ms < block_t0 - EPS_MS)
        {
            discarded.insert(idx);
        }
    }
    discarded
}

// Delete a transport block: drop its play/stop (and any loop) events so the deck
// is silent across [start_ms, end_ms]. A glued predecessor gets a stop so it
// doesn't bleed into the gap; a glued successor keeps its own start. A
// deck_snapshot on the start boundary survives with its transport effects
// cleared (it also loads the track). The track's load is left in place
// (this removes the played segment, not the deck's loaded track) and automation
// is untouched (it lives at wall time).
pub fn delete_transport_block(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
) -> Vec<SessionEvent> {
    let Some(neighborhood) = neighborhood_of(events, clips, block) else {
        return events.to_vec();
    };
    let Neighborhood {
        prev,
        next,
        own_load_ms,
        ..
    } = neighborhood;
    let t0 = block.start_ms;
    let t1 = block.end_ms;
    let deck = block.deck.as_str();
    let prev_glued = prev.as_ref().is_some_and(|p| near(p.end_ms, t0));
    let next_glued = next.as_ref().is_some_and(|n| near(n.start_ms, t1));

    // If this block was the only thing playing from its load_track, the load (and
    // the rate/grid it seeded) is now orphaned: drop it so the deck reads as empty
    // here instead of leaving a stale loaded-span box and label.
    let discarded = orphaned_own_load(events, deck, own_load_ms, t0, t1);

    let mut kept: Vec<SessionEvent> = Vec::new();
    for (idx, event) in events.iter().enumerate() {
        if discarded.contains(&idx) {
            continue;
        }
        // A deck_snapshot on the start boundary loads/seeds the deck; keep it but
        // stop it playing so it no longer opens the block.
        if event.event_type == "deck_snapshot" && near(event.elapsed_ms, t0) {
            if let Some(replacement) = rewrite_start_boundary(event) {
                kept.push(replacement);
            }
            continue;
        }
        if event.deck.as_deref() != Some(deck) || !is_transport(&event.event_type) {
            kept.push(event.clone());
            continue;
        }
        let ms = event.elapsed_ms;
        if ms < t0 - EPS_MS || ms > t1 + EPS_MS {
            kept.push(normalize_resume_play(event, next.as_ref()));
            continue;
        }
        if event.event_type == "load_track" || event.event_type == "eject_track" {
            kept.push(event.clone());
            continue;
        }
        // Inside [t0, t1]: the block's own play/stop/loop events — drop them.
    }
    if prev_glued {
        if let Some(prev) = &prev {
            kept.extend(end_events_for(prev, t0));
        }
    }
    if next_glued {
        if let Some(next) = &next {
            kept.extend(start_events_for(next, t1));
        }
    }
    stable_sort_by_ms(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(deck: &str, start: f64, end: f64, block_id: u32, track_start_sec: f64) -> Clip {
        Clip {
            deck: deck.to_string(),
            session_start_ms: start,
            session_end_ms: end,
            track_path: "/t/a.mp3".to_string(),
            track_start_sec,
            playback_rate: 1.0,
            block_id,
            loop_region: None,
            wave_segments: Vec::new(),
            bpm: None,
            beat_offset_sec: None,
        }
    }

    fn ev(elapsed_ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent::at(elapsed_ms, event_type, deck)
    }

    fn find(events: &[SessionEvent], event_type: &str, ms: f64) -> Option<SessionEvent> {
        events
            .iter()
            .find(|e| e.event_type == event_type && near(e.elapsed_ms, ms))
            .cloned()
    }

    #[test]
    fn blocks_for_deck_groups_and_sorts() {
        let clips = vec![
            clip("A", 4000.0, 6000.0, 1, 0.0),
            clip("A", 1000.0, 3000.0, 0, 0.0),
            clip("B", 0.0, 500.0, 2, 0.0),
        ];
        let blocks = blocks_for_deck(&clips, "A");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].start_ms, 1000.0);
        assert_eq!(blocks[0].block_id, 0);
        assert_eq!(blocks[1].start_ms, 4000.0);
    }

    #[test]
    fn block_bounds_open_ended_for_standalone_block() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();
        let (min_start, max_end) = block_bounds(&events, &clips, &block).unwrap();
        assert_eq!(min_start, 0.0);
        assert_eq!(max_end, f64::INFINITY);
    }

    #[test]
    fn move_shifts_block_and_rewrites_boundaries() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let result = move_transport_block(&events, &clips, &block, 1000.0);
        assert_eq!(result.applied_delta_ms, 1000.0);
        // load stays put; play moves to 2000 (sec 0); stop moves to 6000.
        assert!(find(&result.events, "load_track", 0.0).is_some());
        let play = find(&result.events, "play", 2000.0).unwrap();
        assert_eq!(play.sec, Some(0.0));
        assert!(find(&result.events, "stop", 6000.0).is_some());
        assert!(find(&result.events, "play", 1000.0).is_none());
        assert!(find(&result.events, "stop", 5000.0).is_none());
    }

    #[test]
    fn move_is_clamped_by_next_block() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(3000.0, "stop", "A"),
            ev(4000.0, "play", "A"),
            ev(6000.0, "stop", "A"),
        ];
        let clips = vec![
            clip("A", 1000.0, 3000.0, 0, 0.0),
            clip("A", 4000.0, 6000.0, 1, 0.0),
        ];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        // Asking +2000 would overlap the next block at 4000; clamp to +1000.
        let result = move_transport_block(&events, &clips, &block, 2000.0);
        assert_eq!(result.applied_delta_ms, 1000.0);
    }

    #[test]
    fn move_below_one_ms_is_noop() {
        let events = vec![ev(1000.0, "play", "A"), ev(5000.0, "stop", "A")];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();
        let result = move_transport_block(&events, &clips, &block, 0.4);
        assert_eq!(result.applied_delta_ms, 0.0);
        assert_eq!(result.events.len(), events.len());
    }

    #[test]
    fn trim_end_shortens_block() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let result = trim_transport_block(&events, &clips, &block, Edge::End, 3000.0);
        assert_eq!(result.applied_ms, 3000.0);
        assert!(find(&result.events, "stop", 3000.0).is_some());
        assert!(find(&result.events, "stop", 5000.0).is_none());
        assert!(find(&result.events, "play", 1000.0).is_some());
    }

    #[test]
    fn trim_start_moves_in_and_sets_new_sec() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let result = trim_transport_block(&events, &clips, &block, Edge::Start, 2000.0);
        assert_eq!(result.applied_ms, 2000.0);
        let play = find(&result.events, "play", 2000.0).unwrap();
        // 1s of audio elapses between t0=1000 and applied=2000 at rate 1.
        assert!((play.sec.unwrap() - 1.0).abs() < 1e-6);
        assert!(find(&result.events, "play", 1000.0).is_none());
    }

    #[test]
    fn trim_loop_block_is_rejected() {
        let mut c = clip("A", 1000.0, 5000.0, 0, 4.0);
        c.loop_region = Some(LoopRegion {
            start_sec: 4.0,
            end_sec: 6.0,
        });
        let clips = vec![c];
        let events = vec![ev(1000.0, "play", "A"), ev(5000.0, "stop", "A")];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let result = trim_transport_block(&events, &clips, &block, Edge::Start, 2000.0);
        assert_eq!(result.applied_ms, 1000.0);
        assert_eq!(result.events.len(), events.len());
    }

    #[test]
    fn delete_removes_block_and_its_orphaned_load() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let out = delete_transport_block(&events, &clips, &block);

        assert!(find(&out, "play", 1000.0).is_none());
        assert!(find(&out, "stop", 5000.0).is_none());
        // The only block using the load is gone, so the load is dropped too.
        assert!(!out.iter().any(|e| e.event_type == "load_track"));
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert!(rebuilt.is_empty());
    }

    #[test]
    fn delete_keeps_load_shared_by_another_block() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(3000.0, "stop", "A"),
            ev(6000.0, "play", "A"),
            ev(8000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 3000.0, 0, 0.0), clip("A", 6000.0, 8000.0, 1, 5.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let out = delete_transport_block(&events, &clips, &block);
        // A later block still plays from the same load, so the load survives.
        assert!(out.iter().any(|e| e.event_type == "load_track"));
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert_eq!(rebuilt.len(), 1);
    }

    #[test]
    fn delete_glued_predecessor_gets_a_stop() {
        // Two back-to-back blocks (no stop between); deleting the second must stop
        // the first at the boundary so it doesn't bleed into the gap.
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(3000.0, "stop", "A"),
            ev(3000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
        ];
        let clips = vec![clip("A", 1000.0, 3000.0, 0, 0.0), clip("A", 3000.0, 5000.0, 1, 2.0)];
        let block = blocks_for_deck(&clips, "A")[1].clone();

        let out = delete_transport_block(&events, &clips, &block);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert_eq!(rebuilt.len(), 1);
        assert!((rebuilt[0].session_end_ms - 3000.0).abs() < 1.0);
    }
}
