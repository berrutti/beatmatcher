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
fn start_events_for(block: &TransportBlock, ms: f64) -> Vec<SessionEvent> {
    if let Some(region) = &block.loop_region {
        // track_start_sec is the wrapped entry position, which may sit inside the
        // region; playing from the loop start instead would shift the block.
        vec![
            SessionEvent {
                sec: Some(block.track_start_sec),
                ..SessionEvent::at(ms, "play", &block.deck)
            },
            SessionEvent {
                start_sec: Some(region.start_sec),
                end_sec: Some(region.end_sec),
                ..SessionEvent::at(ms, "loop_out", &block.deck)
            },
        ]
    } else {
        vec![SessionEvent {
            sec: Some(block.track_start_sec),
            ..SessionEvent::at(ms, "play", &block.deck)
        }]
    }
}

fn end_events_for(block: &TransportBlock, ms: f64) -> Vec<SessionEvent> {
    if block.loop_region.is_some() {
        // exit_loop, not a bare stop: a glued loop block must be disarmed, else
        // the relocated clip would wrap at the stale loop boundary.
        vec![
            SessionEvent::at(ms, "exit_loop", &block.deck),
            SessionEvent::at(ms, "stop", &block.deck),
        ]
    } else {
        vec![SessionEvent::at(ms, "stop", &block.deck)]
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
// up to the next load, are dropped. "Played" is decided from the rendered
// clips, not from play events, so a cue-preview clip counts. Returns indices
// into `events`.
fn orphaned_load_events(
    events: &[SessionEvent],
    clips: &[Clip],
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
        let was_played = clips.iter().any(|c| {
            c.deck == deck
                && c.session_start_ms >= load.elapsed_ms - EPS_MS
                && c.session_start_ms < next_load_ms - EPS_MS
        });
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

    let discarded = orphaned_load_events(events, clips, deck, own_load_ms, new_start, new_end);
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
            // A stranded transport event (paused-scrub seek) under the moved
            // block would split it; consume it. normalize_resume_play below
            // re-anchors the next block, so nothing downstream depends on it.
            let swept = ms > new_start + EPS_MS
                && ms < new_end - EPS_MS
                && event.event_type != "load_track"
                && event.event_type != "eject_track";
            if swept {
                continue;
            }
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
            // A stranded transport event inside the extension would split it.
            let swept = event.elapsed_ms > applied + EPS_MS
                && event.elapsed_ms < t0 - EPS_MS
                && event.event_type != "load_track"
                && event.event_type != "eject_track";
            if swept {
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
        // Same sweep as the start edge.
        let swept = event.elapsed_ms > t1 + EPS_MS
            && event.elapsed_ms < applied - EPS_MS
            && event.event_type != "load_track"
            && event.event_type != "eject_track";
        if swept {
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
// deleted, but only when no OTHER clip plays from that load, so a shared track
// stays. Sharing is decided from the rendered clips, not from play events:
// blocks split off by a seek (or opened by a cue preview) have no play event of
// their own, and counting plays alone dropped the load out from under them.
fn orphaned_own_load(
    events: &[SessionEvent],
    clips: &[Clip],
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
    let shared = clips.iter().any(|c| {
        c.deck == deck
            && c.session_start_ms >= own - EPS_MS
            && c.session_start_ms < next_load - EPS_MS
            && (c.session_start_ms < block_t0 - EPS_MS || c.session_end_ms > block_t1 + EPS_MS)
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
    let discarded = orphaned_own_load(events, clips, deck, own_load_ms, t0, t1);

    let mut kept: Vec<SessionEvent> = Vec::new();
    for (idx, event) in events.iter().enumerate() {
        if discarded.contains(&idx) {
            continue;
        }
        // A deck_snapshot on the start boundary loads/seeds the deck; keep it but
        // stop it playing so it no longer opens the block. Own deck only: every
        // deck's snapshot sits at session start, so a block starting at t=0 must
        // not neuter the other decks' snapshots.
        if event.deck.as_deref() == Some(deck)
            && event.event_type == "deck_snapshot"
            && near(event.elapsed_ms, t0)
        {
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
        // Inside [t0, t1]: the block's own play/stop/loop events. Drop them.
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

// Silence one block over [start_ms, end_ms] (clamped to the block). Covering
// the whole block is a full delete; touching an edge is a trim; an interior
// range splits the block in two, the right part starting exactly on the audio
// it played before, so a deleted mid-block region never shifts what follows.
// A remainder shorter than MIN_BLOCK_MS is absorbed into the deletion. Loop
// blocks re-enter their loop at the exact in-loop position the deck had, so
// the surviving iterations keep their original phase.
pub fn delete_block_range(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
    start_ms: f64,
    end_ms: f64,
) -> Vec<SessionEvent> {
    let t0 = block.start_ms;
    let t1 = block.end_ms;
    let range_start = start_ms.max(t0);
    let range_end = end_ms.min(t1);
    if range_end - range_start <= EPS_MS {
        return events.to_vec();
    }
    let left_rest = range_start - t0;
    let right_rest = t1 - range_end;
    if left_rest < MIN_BLOCK_MS && right_rest < MIN_BLOCK_MS {
        return delete_transport_block(events, clips, block);
    }
    if block.loop_region.is_some() {
        let loop_start = if left_rest < MIN_BLOCK_MS {
            t0
        } else {
            range_start
        };
        let loop_end = if right_rest < MIN_BLOCK_MS {
            t1
        } else {
            range_end
        };
        return delete_loop_block_range(events, clips, block, loop_start, loop_end);
    }
    if left_rest < MIN_BLOCK_MS {
        return trim_transport_block(events, clips, block, Edge::Start, range_end).events;
    }
    if right_rest < MIN_BLOCK_MS {
        return trim_transport_block(events, clips, block, Edge::End, range_start).events;
    }
    let deck = block.deck.as_str();
    let resume_sec = (block.track_start_sec
        + audio_seconds_between(events, deck, t0, range_end, block.playback_rate))
    .max(0.0);
    let mut kept = events.to_vec();
    kept.push(SessionEvent::at(range_start, "stop", deck));
    kept.push(SessionEvent {
        sec: Some(resume_sec),
        ..SessionEvent::at(range_end, "play", deck)
    });
    stable_sort_by_ms(kept)
}

// Split a block into two independent blocks at `split_ms`, with no gap: a stop
// immediately followed by a play at the same instant, the right part resuming
// exactly the audio it already played (same construction as delete_block_range's
// interior branch, but with nothing removed). A loop block splits into two
// separate glued engagements of the same loop, the second re-entering at the
// deck's exact in-loop position, so both halves keep their original phase. A
// split within MIN_BLOCK_MS of either edge is rejected: it would leave a
// degenerate sliver on one side.
pub fn split_transport_block(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
    split_ms: f64,
) -> Vec<SessionEvent> {
    let t0 = block.start_ms;
    let t1 = block.end_ms;
    if split_ms - t0 < MIN_BLOCK_MS || t1 - split_ms < MIN_BLOCK_MS {
        return events.to_vec();
    }
    let deck = block.deck.as_str();

    if block.loop_region.is_some() {
        let Some(resume_sec) = loop_position_at(clips, block, split_ms) else {
            return events.to_vec();
        };
        let mut kept = events.to_vec();
        kept.extend(end_events_for(block, split_ms));
        let mut resumed = block.clone();
        resumed.track_start_sec = resume_sec;
        kept.extend(start_events_for(&resumed, split_ms));
        return stable_sort_by_ms(kept);
    }

    let resume_sec = (block.track_start_sec
        + audio_seconds_between(events, deck, t0, split_ms, block.playback_rate))
    .max(0.0);
    let mut kept = events.to_vec();
    kept.push(SessionEvent::at(split_ms, "stop", deck));
    kept.push(SessionEvent {
        sec: Some(resume_sec),
        ..SessionEvent::at(split_ms, "play", deck)
    });
    stable_sort_by_ms(kept)
}

// The deck's track position at wall time `ms` inside a looping block, read off
// the iteration clips' segment mapping (each iteration is one constant-rate
// segment), so loop wrapping is already accounted for.
fn loop_position_at(clips: &[Clip], block: &TransportBlock, ms: f64) -> Option<f64> {
    let clip = clips.iter().find(|c| {
        c.deck == block.deck
            && c.block_id == block.block_id
            && ms >= c.session_start_ms - EPS_MS
            && ms <= c.session_end_ms + EPS_MS
    })?;
    let seg = clip.wave_segments.first()?;
    let wall = seg.wall_end_ms - seg.wall_start_ms;
    if wall <= 0.0 {
        return Some(seg.track_start_sec);
    }
    let frac = ((ms - seg.wall_start_ms) / wall).clamp(0.0, 1.0);
    Some(seg.track_start_sec + frac * (seg.track_end_sec - seg.track_start_sec))
}

// Range delete on a loop block. Deleting up to the block's start edge drops the
// engagement and re-engages at the range end; up to the end edge exits the loop
// at the range start; an interior range does both, splitting the run in two.
// The re-engagement plays from the deck's exact in-loop position at that time,
// so surviving iterations keep their original phase.
fn delete_loop_block_range(
    events: &[SessionEvent],
    clips: &[Clip],
    block: &TransportBlock,
    range_start: f64,
    range_end: f64,
) -> Vec<SessionEvent> {
    let Some(neighborhood) = neighborhood_of(events, clips, block) else {
        return events.to_vec();
    };
    let t0 = block.start_ms;
    let t1 = block.end_ms;
    let deck = block.deck.as_str();
    let left_trim = range_start <= t0 + EPS_MS;
    let right_trim = range_end >= t1 - EPS_MS;

    let resume_sec = if right_trim {
        None
    } else {
        match loop_position_at(clips, block, range_end) {
            Some(sec) => Some(sec),
            None => return events.to_vec(),
        }
    };

    let mut kept: Vec<SessionEvent> = Vec::new();
    for event in events {
        // The own-deck snapshot on a dropped start boundary seeds the deck; keep
        // it but stop it playing (same rule as the full delete).
        if left_trim
            && event.deck.as_deref() == Some(deck)
            && event.event_type == "deck_snapshot"
            && near(event.elapsed_ms, t0)
        {
            if let Some(replacement) = rewrite_start_boundary(event) {
                kept.push(replacement);
            }
            continue;
        }
        if event.deck.as_deref() != Some(deck) || !is_transport(&event.event_type) {
            kept.push(event.clone());
            continue;
        }
        let on_dropped_boundary = (left_trim && near(event.elapsed_ms, t0))
            || (right_trim && near(event.elapsed_ms, t1));
        if on_dropped_boundary
            && event.event_type != "load_track"
            && event.event_type != "eject_track"
        {
            continue;
        }
        kept.push(normalize_resume_play(event, neighborhood.next.as_ref()));
    }

    if left_trim {
        if let Some(prev) = &neighborhood.prev {
            if near(prev.end_ms, t0) {
                kept.extend(end_events_for(prev, t0));
            }
        }
    } else {
        kept.extend(end_events_for(block, range_start));
    }
    if right_trim {
        if let Some(next) = &neighborhood.next {
            if near(next.start_ms, t1) {
                kept.extend(start_events_for(next, t1));
            }
        }
    } else if let Some(sec) = resume_sec {
        let mut resumed = block.clone();
        resumed.track_start_sec = sec;
        kept.extend(start_events_for(&resumed, range_end));
    }
    stable_sort_by_ms(kept)
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRange {
    pub deck: String,
    pub start_ms: f64,
    pub end_ms: f64,
}

// Delete several ranges as one edit. Ranges are applied right-to-left: a
// delete never moves anything to its left (full deletes and splits leave every
// other block byte-identical; edge trims only move the trimmed boundary), so
// blocks for the remaining targets are re-located by span in the rebuilt
// clips. A range spanning several blocks is clipped to each.
pub fn delete_transport_ranges(
    events: &[SessionEvent],
    clips: &[Clip],
    ranges: &[DeleteRange],
) -> Vec<SessionEvent> {
    let mut events = events.to_vec();
    let mut clips = clips.to_vec();
    let mut sorted: Vec<DeleteRange> = ranges.to_vec();
    sorted.sort_by(|a, b| {
        b.start_ms
            .partial_cmp(&a.start_ms)
            .unwrap_or(Ordering::Equal)
    });
    for range in &sorted {
        let mut subs: Vec<(f64, f64)> = blocks_for_deck(&clips, &range.deck)
            .into_iter()
            .filter(|b| b.end_ms > range.start_ms + EPS_MS && b.start_ms < range.end_ms - EPS_MS)
            .map(|b| (b.start_ms.max(range.start_ms), b.end_ms.min(range.end_ms)))
            .collect();
        subs.reverse();
        for &(sub_start, sub_end) in &subs {
            let mid = (sub_start + sub_end) / 2.0;
            let Some(block) = blocks_for_deck(&clips, &range.deck)
                .into_iter()
                .find(|b| mid > b.start_ms - EPS_MS && mid < b.end_ms + EPS_MS)
            else {
                continue;
            };
            events = delete_block_range(&events, &clips, &block, sub_start, sub_end);
            clips = crate::timeline::build_clips(&events).clips;
        }
    }
    events
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

    fn range(deck: &str, start_ms: f64, end_ms: f64) -> DeleteRange {
        DeleteRange {
            deck: deck.to_string(),
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn delete_ranges_removes_whole_blocks_and_leaves_the_rest_untouched() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(2000.0, "stop", "A"),
            ev(3000.0, "play", "A"),
            ev(4000.0, "stop", "A"),
            ev(5000.0, "play", "A"),
            ev(6000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);

        let out = delete_transport_ranges(
            &events,
            &clips,
            &[range("A", 1000.0, 2000.0), range("A", 5000.0, 6000.0)],
        );
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert_eq!(rebuilt.len(), 1);
        assert!((rebuilt[0].session_start_ms - 3000.0).abs() < 1.0);
        assert!((rebuilt[0].session_end_ms - 4000.0).abs() < 1.0);
        assert!((rebuilt[0].track_start_sec - 1.0).abs() < 1e-6);
    }

    #[test]
    fn delete_ranges_covering_a_whole_load_drops_it() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            SessionEvent {
                sec: Some(30.0),
                ..ev(5000.0, "seek", "A")
            },
            ev(9000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);

        // One range across both seek-split blocks, clipped to each.
        let out = delete_transport_ranges(&events, &clips, &[range("A", 1000.0, 9000.0)]);
        assert!(!out.iter().any(|e| e.event_type == "load_track"));
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert!(rebuilt.is_empty());
    }

    #[test]
    fn delete_interior_range_splits_the_block_and_keeps_the_right_part_aligned() {
        // 127->128-style scenario in one continuous play: rate changes at 60s;
        // deleting the earlier region must leave the later region playing the
        // exact audio it played before, at its original session position.
        let r1 = 1.0583;
        let r2 = 1.0667;
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            SessionEvent {
                rate: Some(r1),
                ..ev(1.0, "set_playback_rate", "A")
            },
            ev(10_000.0, "play", "A"),
            SessionEvent {
                rate: Some(r2),
                ..ev(60_000.0, "set_playback_rate", "A")
            },
            ev(120_000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        assert_eq!(clips.len(), 1);
        let seg_before = clips[0].wave_segments.last().unwrap().clone();

        // Delete the r1 region except its first 20s (an interior range).
        let out = delete_transport_ranges(&events, &clips, &[range("A", 30_000.0, 60_000.0)]);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert_eq!(rebuilt.len(), 2);
        assert!((rebuilt[0].session_start_ms - 10_000.0).abs() < 1.0);
        assert!((rebuilt[0].session_end_ms - 30_000.0).abs() < 1.0);
        // The right part resumes at 60s exactly where the audio would have
        // been: 50s of wall time at r1 past the original start.
        assert!((rebuilt[1].session_start_ms - 60_000.0).abs() < 1.0);
        assert!((rebuilt[1].track_start_sec - 50.0 * r1).abs() < 1e-6);
        let seg_after = rebuilt[1].wave_segments.last().unwrap();
        assert!((seg_after.wall_start_ms - seg_before.wall_start_ms).abs() < 1.0);
        assert!((seg_after.track_start_sec - seg_before.track_start_sec).abs() < 1e-6);
        assert!((seg_after.track_end_sec - seg_before.track_end_sec).abs() < 1e-6);
    }

    #[test]
    fn split_gaplessly_divides_a_block_at_the_split_point() {
        let r1 = 1.0583;
        let r2 = 1.0667;
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            SessionEvent {
                rate: Some(r1),
                ..ev(1.0, "set_playback_rate", "A")
            },
            ev(10_000.0, "play", "A"),
            SessionEvent {
                rate: Some(r2),
                ..ev(60_000.0, "set_playback_rate", "A")
            },
            ev(120_000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let out = split_transport_block(&events, &clips, &block, 30_000.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert_eq!(rebuilt.len(), 2);
        assert!((rebuilt[0].session_start_ms - 10_000.0).abs() < 1.0);
        assert!((rebuilt[0].session_end_ms - 30_000.0).abs() < 1.0);
        // No gap: the right part starts exactly at the split point.
        assert!((rebuilt[1].session_start_ms - 30_000.0).abs() < 1.0);
        assert!((rebuilt[1].session_end_ms - 120_000.0).abs() < 1.0);
        // 20s of wall time at r1 past the original start.
        assert!((rebuilt[1].track_start_sec - 20.0 * r1).abs() < 1e-6);
    }

    #[test]
    fn split_too_close_to_an_edge_is_rejected() {
        let events = vec![ev(1000.0, "play", "A"), ev(5000.0, "stop", "A")];
        let clips = vec![clip("A", 1000.0, 5000.0, 0, 0.0)];
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let out = split_transport_block(&events, &clips, &block, 1050.0);
        assert_eq!(out.len(), events.len());
        let out = split_transport_block(&events, &clips, &block, 4950.0);
        assert_eq!(out.len(), events.len());
    }

    // A session with a loop run: play from 0, loop 0..2s engaged at 2000 (the
    // playhead is exactly at the loop end, so it wraps to 0), exit at 8000,
    // regular tail until 10000. Iterations: [2000,4000) [4000,6000) [6000,8000),
    // each mapping 0..2s.
    fn looped_session() -> Vec<SessionEvent> {
        vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(0.0, "play", "A"),
            SessionEvent {
                start_sec: Some(0.0),
                end_sec: Some(2.0),
                ..ev(2000.0, "loop_out", "A")
            },
            ev(8000.0, "exit_loop", "A"),
            ev(10_000.0, "stop", "A"),
        ]
    }

    fn loop_block_of(clips: &[Clip]) -> TransportBlock {
        blocks_for_deck(clips, "A")
            .into_iter()
            .find(|b| b.loop_region.is_some())
            .unwrap()
    }

    #[test]
    fn delete_interior_loop_range_splits_the_run_and_keeps_phase() {
        let events = looped_session();
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = loop_block_of(&clips);

        // Delete [3000, 6500]: the run resumes mid-iteration at 0.5s in-loop.
        let out = delete_block_range(&events, &clips, &block, 3000.0, 6500.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert!(!rebuilt
            .iter()
            .any(|c| c.session_start_ms > 3001.0 && c.session_start_ms < 6499.0));
        let resumed = rebuilt
            .iter()
            .find(|c| (c.session_start_ms - 6500.0).abs() < 1.0)
            .unwrap();
        assert!(resumed.loop_region.is_some());
        assert!((resumed.track_start_sec - 0.5).abs() < 1e-6);
        assert!((resumed.session_end_ms - 8000.0).abs() < 1.0);
        // The tail after the loop is untouched and still starts at track 0.
        let tail = rebuilt
            .iter()
            .find(|c| (c.session_start_ms - 8000.0).abs() < 1.0)
            .unwrap();
        assert!(tail.loop_region.is_none());
        assert!(tail.track_start_sec.abs() < 1e-6);
    }

    #[test]
    fn split_loop_block_divides_the_run_gaplessly_at_the_original_phase() {
        let events = looped_session();
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = loop_block_of(&clips);

        // Split at 6500: 0.5s into the last iteration, which started at 6000.
        let out = split_transport_block(&events, &clips, &block, 6500.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        let left_end = rebuilt
            .iter()
            .filter(|c| c.loop_region.is_some() && c.session_end_ms <= 6500.0 + 1.0)
            .map(|c| c.session_end_ms)
            .fold(0.0_f64, f64::max);
        assert!((left_end - 6500.0).abs() < 1.0);
        let resumed = rebuilt
            .iter()
            .find(|c| (c.session_start_ms - 6500.0).abs() < 1.0)
            .unwrap();
        assert!(resumed.loop_region.is_some());
        assert!((resumed.track_start_sec - 0.5).abs() < 1e-6);
        assert!((resumed.session_end_ms - 8000.0).abs() < 1.0);
        // The tail after the loop is untouched.
        let tail = rebuilt
            .iter()
            .find(|c| (c.session_start_ms - 8000.0).abs() < 1.0)
            .unwrap();
        assert!(tail.loop_region.is_none());
    }

    #[test]
    fn delete_loop_start_range_reengages_later_at_the_original_phase() {
        let events = looped_session();
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = loop_block_of(&clips);

        // Delete [2000, 5000]: the glued pre-loop clip gets a stop at 2000 and
        // the loop re-engages at 5000, 1.0s into the region.
        let out = delete_block_range(&events, &clips, &block, 2000.0, 5000.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        let pre = &rebuilt[0];
        assert!(pre.loop_region.is_none());
        assert!((pre.session_end_ms - 2000.0).abs() < 1.0);
        let resumed = rebuilt
            .iter()
            .find(|c| (c.session_start_ms - 5000.0).abs() < 1.0)
            .unwrap();
        assert!(resumed.loop_region.is_some());
        assert!((resumed.track_start_sec - 1.0).abs() < 1e-6);
    }

    #[test]
    fn delete_loop_end_range_exits_early_and_keeps_the_glued_tail_aligned() {
        let events = looped_session();
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = loop_block_of(&clips);

        // Delete [5000, 8000]: the loop exits at 5000; the glued tail at 8000
        // still plays exactly the audio it played before.
        let out = delete_block_range(&events, &clips, &block, 5000.0, 8000.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        let last_loop = rebuilt
            .iter()
            .filter(|c| c.loop_region.is_some())
            .last()
            .unwrap();
        assert!((last_loop.session_end_ms - 5000.0).abs() < 1.0);
        assert!(!rebuilt
            .iter()
            .any(|c| c.session_start_ms > 5001.0 && c.session_start_ms < 7999.0));
        let tail = rebuilt
            .iter()
            .find(|c| (c.session_start_ms - 8000.0).abs() < 1.0)
            .unwrap();
        assert!(tail.loop_region.is_none());
        assert!(tail.track_start_sec.abs() < 1e-6);
        assert!((tail.session_end_ms - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn delete_edge_ranges_trim_and_tiny_remainders_are_absorbed() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(9000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        // Range touching the start trims it; audio stays aligned in session time.
        let out = delete_block_range(&events, &clips, &block, 1000.0, 3000.0);
        let crate::timeline::ClipsBuild { clips: trimmed, .. } = crate::timeline::build_clips(&out);
        assert_eq!(trimmed.len(), 1);
        assert!((trimmed[0].session_start_ms - 3000.0).abs() < 1.0);
        assert!((trimmed[0].track_start_sec - 2.0).abs() < 1e-6);

        // A remainder below MIN_BLOCK_MS is absorbed: this is a whole delete.
        let out = delete_block_range(&events, &clips, &block, 1050.0, 8950.0);
        let crate::timeline::ClipsBuild { clips: gone, .. } = crate::timeline::build_clips(&out);
        assert!(gone.is_empty());
    }

    #[test]
    fn move_keeps_swept_load_played_only_via_cue_preview() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(3000.0, "stop", "A"),
            SessionEvent {
                path: Some("/t/b.mp3".to_string()),
                ..ev(4000.0, "load_track", "A")
            },
            SessionEvent {
                cue_point_sec: Some(0.0),
                ..ev(6000.0, "cue_preview_start", "A")
            },
            SessionEvent {
                cue_point_sec: Some(0.0),
                ..ev(7000.0, "cue_preview_end", "A")
            },
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        // +5000 is clamped by the preview clip at 6000, but the moved span still
        // sweeps the load at 4000; the previewed track's load must survive.
        let result = move_transport_block(&events, &clips, &block, 5000.0);
        assert!(result.applied_delta_ms > 0.0);
        assert!(result
            .events
            .iter()
            .any(|e| e.event_type == "load_track" && e.path.as_deref() == Some("/t/b.mp3")));
    }

    #[test]
    fn delete_keeps_load_for_seek_continuation_blocks() {
        // One play, blocks split by a seek: the later block has no play event of
        // its own, so the load must still count as shared when the first block
        // is deleted.
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            SessionEvent {
                sec: Some(30.0),
                ..ev(5000.0, "seek", "A")
            },
            ev(9000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let out = delete_transport_block(&events, &clips, &block);
        assert!(out.iter().any(|e| e.event_type == "load_track"));
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert_eq!(rebuilt.len(), 1);
        assert!((rebuilt[0].session_start_ms - 5000.0).abs() < 1.0);
        assert!((rebuilt[0].session_end_ms - 9000.0).abs() < 1.0);
        assert!((rebuilt[0].track_start_sec - 30.0).abs() < 1e-6);
    }

    #[test]
    fn delete_does_not_clobber_another_decks_snapshot() {
        let snap = |deck: &str, path: &str, pos: f64| SessionEvent {
            path: Some(path.to_string()),
            position_sec: Some(pos),
            cue_point_sec: Some(pos),
            is_playing: Some(true),
            loop_active: Some(false),
            playback_rate: Some(1.0),
            ..ev(0.0, "deck_snapshot", deck)
        };
        let events = vec![
            snap("A", "/t/a.mp3", 10.0),
            snap("B", "/t/b.mp3", 20.0),
            ev(30_000.0, "stop", "A"),
            ev(60_000.0, "stop", "B"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let out = delete_transport_block(&events, &clips, &block);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } = crate::timeline::build_clips(&out);
        assert!(rebuilt.iter().all(|c| c.deck != "A"));
        let deck_b: Vec<&crate::timeline::Clip> =
            rebuilt.iter().filter(|c| c.deck == "B").collect();
        assert_eq!(deck_b.len(), 1);
        assert!((deck_b[0].session_end_ms - 60_000.0).abs() < 1.0);
        assert!((deck_b[0].track_start_sec - 20.0).abs() < 1e-6);
    }

    // The gesture clamp allows dragging an end edge exactly onto next.start_ms.
    #[test]
    fn trim_end_flush_against_next_block_keeps_it() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
            ev(7000.0, "play", "A"),
            ev(9000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let blocks = blocks_for_deck(&clips, "A");
        assert_eq!(blocks.len(), 2);

        let result = trim_transport_block(&events, &clips, &blocks[0], Edge::End, 7000.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } =
            crate::timeline::build_clips(&result.events);
        let rebuilt_blocks = blocks_for_deck(&rebuilt, "A");
        assert_eq!(
            rebuilt_blocks.len(),
            2,
            "extending block 1 flush against block 2 swallowed block 2: {rebuilt_blocks:?}"
        );
        let second = &rebuilt_blocks[1];
        assert!((second.start_ms - 7000.0).abs() < 1.0);
        assert!((second.end_ms - 9000.0).abs() < 1.0);
        assert!((second.track_start_sec - 4.0).abs() < 1e-6);
    }

    // Paused-scrub seeks are logged, so silent gaps contain transport events.
    #[test]
    fn trim_end_over_stray_seek_keeps_audio_continuous() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(5000.0, "stop", "A"),
            SessionEvent {
                sec: Some(30.0),
                ..ev(6000.0, "seek", "A")
            },
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let result = trim_transport_block(&events, &clips, &block, Edge::End, 8000.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } =
            crate::timeline::build_clips(&result.events);
        assert_eq!(
            rebuilt.len(),
            1,
            "stray seek inside the extension split the clip: {rebuilt:?}"
        );
        assert!((rebuilt[0].session_end_ms - 8000.0).abs() < 1.0);
        assert!(rebuilt[0].track_start_sec.abs() < 1e-6);
    }

    #[test]
    fn move_over_stray_seek_keeps_audio_continuous() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(3000.0, "stop", "A"),
            SessionEvent {
                sec: Some(30.0),
                ..ev(6000.0, "seek", "A")
            },
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let block = blocks_for_deck(&clips, "A")[0].clone();

        let result = move_transport_block(&events, &clips, &block, 4000.0);
        assert_eq!(result.applied_delta_ms, 4000.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } =
            crate::timeline::build_clips(&result.events);
        assert_eq!(
            rebuilt.len(),
            1,
            "stray seek under the moved block split the clip: {rebuilt:?}"
        );
        assert!((rebuilt[0].session_start_ms - 5000.0).abs() < 1.0);
        assert!((rebuilt[0].session_end_ms - 7000.0).abs() < 1.0);
        assert!(rebuilt[0].track_start_sec.abs() < 1e-6);
    }

    #[test]
    fn move_sweep_keeps_downstream_resume_anchored() {
        let events = vec![
            SessionEvent {
                path: Some("/t/a.mp3".to_string()),
                ..ev(0.0, "load_track", "A")
            },
            ev(1000.0, "play", "A"),
            ev(3000.0, "stop", "A"),
            SessionEvent {
                sec: Some(30.0),
                ..ev(4000.0, "seek", "A")
            },
            ev(6000.0, "play", "A"),
            ev(8000.0, "stop", "A"),
        ];
        let crate::timeline::ClipsBuild { clips, .. } = crate::timeline::build_clips(&events);
        let blocks = blocks_for_deck(&clips, "A");
        assert_eq!(blocks.len(), 2);
        assert!((blocks[1].track_start_sec - 30.0).abs() < 1e-6);

        // +1500 lands block 1 on [2500, 4500], sweeping the seek at 4000.
        let result = move_transport_block(&events, &clips, &blocks[0], 1500.0);
        assert_eq!(result.applied_delta_ms, 1500.0);
        let crate::timeline::ClipsBuild { clips: rebuilt, .. } =
            crate::timeline::build_clips(&result.events);
        let rebuilt_blocks = blocks_for_deck(&rebuilt, "A");
        assert_eq!(rebuilt_blocks.len(), 2, "blocks: {rebuilt_blocks:?}");
        assert!((rebuilt_blocks[0].start_ms - 2500.0).abs() < 1.0);
        assert!((rebuilt_blocks[0].end_ms - 4500.0).abs() < 1.0);
        assert!(rebuilt_blocks[0].track_start_sec.abs() < 1e-6);
        let moved_clips: Vec<&Clip> = rebuilt
            .iter()
            .filter(|c| c.session_start_ms < 4500.0)
            .collect();
        assert_eq!(moved_clips.len(), 1, "swept seek split the moved block");
        assert!((rebuilt_blocks[1].start_ms - 6000.0).abs() < 1.0);
        assert!((rebuilt_blocks[1].end_ms - 8000.0).abs() < 1.0);
        assert!((rebuilt_blocks[1].track_start_sec - 30.0).abs() < 1e-6);
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

// Randomised sweeps over the edit operations. Block geometry interacts with neighbours,
// loops and rate changes in combinations impractical to enumerate by hand.
#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::timeline::build_clips;

    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    // A plausible recorded session: two decks loading, playing, adjusting and stopping.
    fn make_session(seed: &mut u64) -> Vec<SessionEvent> {
        let mut events: Vec<SessionEvent> = Vec::new();
        let mut t = 0.0f64;
        for deck in ["A", "B"] {
            t += (rng(seed) % 500) as f64;
            events.push(SessionEvent {
                path: Some(format!("/music/{deck}.wav")),
                duration: Some(300.0),
                beat_offset_sec: Some(0.0),
                ..SessionEvent::at(t, "load_track", deck)
            });
            t += (rng(seed) % 500) as f64;
            events.push(SessionEvent {
                sec: Some(0.0),
                ..SessionEvent::at(t, "play", deck)
            });
            for _ in 0..(rng(seed) % 4) {
                t += 200.0 + (rng(seed) % 3000) as f64;
                match rng(seed) % 3 {
                    0 => events.push(SessionEvent::param(
                        t,
                        Some(deck),
                        "fader",
                        "gain",
                        (rng(seed) % 100) as f64 / 100.0,
                    )),
                    1 => events.push(SessionEvent {
                        rate: Some(0.95 + (rng(seed) % 10) as f64 / 100.0),
                        ..SessionEvent::at(t, "set_playback_rate", deck)
                    }),
                    _ => events.push(SessionEvent {
                        sec: Some((rng(seed) % 60) as f64),
                        ..SessionEvent::at(t, "seek", deck)
                    }),
                }
            }
            t += 1000.0 + (rng(seed) % 5000) as f64;
            events.push(SessionEvent::at(t, "stop", deck));
        }
        events.sort_by(crate::sim::event_sim_order);
        events
    }

    struct Report {
        unsorted: u32,
        non_finite: u32,
        negative_ms: u32,
        inverted_clip: u32,
        examples: Vec<String>,
    }

    fn check(label: &str, out: &[SessionEvent], rep: &mut Report, ctx: &str) {
        for w in out.windows(2) {
            if w[0].elapsed_ms > w[1].elapsed_ms {
                rep.unsorted += 1;
                if rep.examples.len() < 4 {
                    rep.examples.push(format!("{label} UNSORTED: {ctx}"));
                }
                break;
            }
        }
        for e in out {
            if !e.elapsed_ms.is_finite() {
                rep.non_finite += 1;
                if rep.examples.len() < 4 {
                    rep.examples.push(format!("{label} NON-FINITE ms: {ctx}"));
                }
                break;
            }
            if e.elapsed_ms < 0.0 {
                rep.negative_ms += 1;
                if rep.examples.len() < 4 {
                    rep.examples
                        .push(format!("{label} NEGATIVE ms {}: {ctx}", e.elapsed_ms));
                }
                break;
            }
        }
        let rebuilt = build_clips(out);
        for c in &rebuilt.clips {
            if !c.session_start_ms.is_finite()
                || !c.session_end_ms.is_finite()
                || c.session_end_ms < c.session_start_ms
            {
                rep.inverted_clip += 1;
                if rep.examples.len() < 4 {
                    rep.examples.push(format!(
                        "{label} BAD CLIP [{}..{}]: {ctx}",
                        c.session_start_ms, c.session_end_ms
                    ));
                }
                break;
            }
        }
    }

    #[test]
    fn fuzz_clip_edit_ops_preserve_structure() {
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut rep = Report {
            unsorted: 0,
            non_finite: 0,
            negative_ms: 0,
            inverted_clip: 0,
            examples: Vec::new(),
        };
        let mut applied = 0u32;

        for _ in 0..4000 {
            let events = make_session(&mut seed);
            let clips = build_clips(&events).clips;
            if clips.is_empty() {
                continue;
            }
            let deck = if rng(&mut seed) % 2 == 0 { "A" } else { "B" };
            let blocks = blocks_for_deck(&clips, deck);
            if blocks.is_empty() {
                continue;
            }
            let block = &blocks[(rng(&mut seed) as usize) % blocks.len()];
            let span = (block.end_ms - block.start_ms).max(1.0);
            applied += 1;

            match rng(&mut seed) % 6 {
                0 => {
                    let delta = (rng(&mut seed) % 8000) as f64 - 4000.0;
                    let out = move_transport_block(&events, &clips, block, delta).events;
                    check("move", &out, &mut rep, &format!("delta={delta}"));
                }
                1 => {
                    let new_ms = block.start_ms + (rng(&mut seed) % (span as u64 * 2 + 1)) as f64
                        - span / 2.0;
                    let out =
                        trim_transport_block(&events, &clips, block, Edge::Start, new_ms).events;
                    check("trim-start", &out, &mut rep, &format!("new_ms={new_ms}"));
                }
                2 => {
                    let new_ms =
                        block.end_ms + (rng(&mut seed) % (span as u64 * 2 + 1)) as f64 - span / 2.0;
                    let out = trim_transport_block(&events, &clips, block, Edge::End, new_ms).events;
                    check("trim-end", &out, &mut rep, &format!("new_ms={new_ms}"));
                }
                3 => {
                    let out = delete_transport_block(&events, &clips, block);
                    check("delete", &out, &mut rep, "");
                }
                4 => {
                    let split = block.start_ms + (rng(&mut seed) % span as u64) as f64;
                    let out = split_transport_block(&events, &clips, block, split);
                    check("split", &out, &mut rep, &format!("split={split}"));
                }
                _ => {
                    let a = block.start_ms + (rng(&mut seed) % span as u64) as f64;
                    let b = block.start_ms + (rng(&mut seed) % span as u64) as f64;
                    let (s, e) = if a <= b { (a, b) } else { (b, a) };
                    let out = delete_block_range(&events, &clips, block, s, e);
                    check("delete-range", &out, &mut rep, &format!("range=[{s},{e}]"));
                }
            }
        }

        println!(
            "CLIP_EDIT fuzz: {applied} ops | unsorted={} non_finite={} negative_ms={} inverted_clip={}",
            rep.unsorted, rep.non_finite, rep.negative_ms, rep.inverted_clip
        );
        for e in &rep.examples {
            println!("   {e}");
        }
        assert_eq!(
            rep.unsorted + rep.non_finite + rep.negative_ms + rep.inverted_clip,
            0
        );
    }

    // Semantic invariants: an edit must actually do what it claims.
    #[test]
    fn fuzz_clip_edit_ops_have_declared_effect() {
        let mut seed = 0x123456789ABCDEFu64;
        let (mut del_bad, mut split_bad, mut move_bad) = (0u32, 0u32, 0u32);
        let (mut del_n, mut split_n, mut move_n) = (0u32, 0u32, 0u32);
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..4000 {
            let events = make_session(&mut seed);
            let clips = build_clips(&events).clips;
            if clips.is_empty() { continue; }
            let deck = if rng(&mut seed) % 2 == 0 { "A" } else { "B" };
            let before = blocks_for_deck(&clips, deck);
            if before.is_empty() { continue; }
            let block = before[(rng(&mut seed) as usize) % before.len()].clone();
            let span = block.end_ms - block.start_ms;

            match rng(&mut seed) % 3 {
                0 => {
                    del_n += 1;
                    let out = delete_transport_block(&events, &clips, &block);
                    let after = blocks_for_deck(&build_clips(&out).clips, deck);
                    if after.len() + 1 != before.len() {
                        del_bad += 1;
                        if examples.len() < 5 {
                            examples.push(format!(
                                "DELETE: {} blocks -> {} (expected {}) span={span}",
                                before.len(), after.len(), before.len() - 1));
                        }
                    }
                }
                1 => {
                    if span < 2.0 * MIN_BLOCK_MS + 2.0 { continue; }
                    let split = block.start_ms + MIN_BLOCK_MS + 1.0
                        + (rng(&mut seed) % (span - 2.0 * MIN_BLOCK_MS - 1.0) as u64) as f64;
                    split_n += 1;
                    let out = split_transport_block(&events, &clips, &block, split);
                    let after = blocks_for_deck(&build_clips(&out).clips, deck);
                    if after.len() != before.len() + 1 {
                        split_bad += 1;
                        if examples.len() < 5 {
                            examples.push(format!(
                                "SPLIT at {split} in [{},{}]: {} blocks -> {} (expected {})",
                                block.start_ms, block.end_ms,
                                before.len(), after.len(), before.len() + 1));
                        }
                    }
                }
                _ => {
                    let delta = (rng(&mut seed) % 4000) as f64 - 2000.0;
                    move_n += 1;
                    let res = move_transport_block(&events, &clips, &block, delta);
                    if res.applied_delta_ms.abs() < EPS_MS { continue; }
                    let target = block.start_ms + res.applied_delta_ms;
                    let after = blocks_for_deck(&build_clips(&res.events).clips, deck);
                    if !after.iter().any(|b| (b.start_ms - target).abs() <= EPS_MS * 2.0) {
                        move_bad += 1;
                        if examples.len() < 5 {
                            let got: Vec<String> =
                                after.iter().map(|b| format!("{:.0}", b.start_ms)).collect();
                            examples.push(format!(
                                "MOVE {} by applied={} -> expected start {target}, got [{}]",
                                block.start_ms, res.applied_delta_ms, got.join(",")));
                        }
                    }
                }
            }
        }

        println!("SEMANTIC fuzz: delete {del_bad}/{del_n} | split {split_bad}/{split_n} | move {move_bad}/{move_n}");
        for e in &examples { println!("   {e}"); }
        assert_eq!(del_bad + split_bad + move_bad, 0);
    }
}
