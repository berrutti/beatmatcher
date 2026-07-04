// Automation-lane editing: draw/clear value gestures (gain/eq/filter/rate),
// toggle the filter on/off over a range, paint/delete nudge spans, and relocate
// track paths. A faithful port of the frontend's sessionEditOps.ts. The TS
// LaneSpec carried closures; here it is an enum (EditableLane) with a resolved
// LaneSpec and matching/value/make logic in methods.

use crate::event::SessionEvent;
use crate::sim::DEFAULT_MASTER_GAIN;
use crate::timeline::{LanePoint, DEFAULT_EQ_DB, DEFAULT_FILTER_VALUE, DEFAULT_GAIN, DEFAULT_RATE};
use std::cmp::Ordering;
use std::collections::HashMap;

// Gestures spanning less time than this are rejected: the value would change and
// restore almost instantly, inaudible and rendering as a bare vertical line.
pub const MIN_GESTURE_MS: f64 = 50.0;

// Public: the native DSP and the frontend mixer read these from here.
pub const EQ_MIN_DB: f64 = -26.0;
pub const EQ_MAX_DB: f64 = 6.0;
pub const FILTER_DEAD_ZONE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditableLane {
    Gain,
    EqLow,
    EqMid,
    EqHigh,
    Filter,
    Rate,
    MasterGain,
}

impl EditableLane {
    pub fn from_key(key: &str) -> Option<EditableLane> {
        Some(match key {
            "gain" => Self::Gain,
            "eqLow" => Self::EqLow,
            "eqMid" => Self::EqMid,
            "eqHigh" => Self::EqHigh,
            "filter" => Self::Filter,
            "rate" => Self::Rate,
            "masterGain" => Self::MasterGain,
            _ => return None,
        })
    }

    fn eq_band(&self) -> Option<&'static str> {
        match self {
            Self::EqLow => Some("low"),
            Self::EqMid => Some("mid"),
            Self::EqHigh => Some("high"),
            _ => None,
        }
    }
}

pub struct LaneSpec {
    pub lane: EditableLane,
    pub min: f64,
    pub max: f64,
    pub default_value: f64,
    pub epsilon: f64,
}

pub fn lane_spec_for(lane: EditableLane, rate_min: Option<f64>, rate_max: Option<f64>) -> LaneSpec {
    let (min, max, default_value, epsilon) = match lane {
        EditableLane::Gain => (0.0, 1.0, DEFAULT_GAIN, 0.01),
        EditableLane::EqLow | EditableLane::EqMid | EditableLane::EqHigh => {
            (EQ_MIN_DB, EQ_MAX_DB, DEFAULT_EQ_DB, 0.25)
        }
        EditableLane::Filter => (-1.0, 1.0, DEFAULT_FILTER_VALUE, 0.01),
        EditableLane::Rate => (
            rate_min.unwrap_or(0.92),
            rate_max.unwrap_or(1.08),
            DEFAULT_RATE,
            0.0005,
        ),
        EditableLane::MasterGain => (0.0, 1.0, DEFAULT_MASTER_GAIN as f64, 0.01),
    };
    LaneSpec {
        lane,
        min,
        max,
        default_value,
        epsilon,
    }
}

impl LaneSpec {
    fn snap(&self, value: f64) -> f64 {
        if self.lane == EditableLane::Filter && value.abs() <= FILTER_DEAD_ZONE {
            0.0
        } else {
            value
        }
    }

    pub fn clamp_value(&self, value: f64) -> f64 {
        self.snap(self.max.min(self.min.max(value)))
    }

    fn matches(&self, event: &SessionEvent, deck: &str) -> bool {
        let deck_ok = event.deck.as_deref() == Some(deck);
        match self.lane {
            EditableLane::Gain => event.event_type == "set_volume" && deck_ok,
            EditableLane::EqLow | EditableLane::EqMid | EditableLane::EqHigh => {
                event.event_type == "set_eq"
                    && deck_ok
                    && event.band.as_deref() == self.lane.eq_band()
            }
            EditableLane::Filter => event.event_type == "set_filter" && deck_ok,
            EditableLane::Rate => event.event_type == "set_playback_rate" && deck_ok,
            EditableLane::MasterGain => event.event_type == "set_master_gain",
        }
    }

    fn value_at(&self, event: &SessionEvent, deck: &str) -> Option<f64> {
        let deck_ok = event.deck.as_deref() == Some(deck);
        match self.lane {
            EditableLane::Gain => (event.event_type == "set_volume" && deck_ok)
                .then(|| event.gain.map(|gain| gain as f64))?,
            EditableLane::EqLow | EditableLane::EqMid | EditableLane::EqHigh => (event.event_type
                == "set_eq"
                && deck_ok
                && event.band.as_deref() == self.lane.eq_band())
            .then(|| event.db.map(|band_db| band_db as f64))?,
            EditableLane::Filter => (event.event_type == "set_filter" && deck_ok)
                .then(|| event.value.map(|value| value as f64))?,
            EditableLane::Rate => {
                if event.event_type == "set_playback_rate" && deck_ok {
                    return event.rate;
                }
                if event.event_type == "deck_snapshot" && deck_ok {
                    return event.playback_rate;
                }
                None
            }
            EditableLane::MasterGain => {
                (event.event_type == "set_master_gain").then(|| event.gain.map(|gain| gain as f64))?
            }
        }
    }

    fn make_event(&self, ms: f64, value: f64, deck: &str) -> SessionEvent {
        match self.lane {
            EditableLane::Gain => SessionEvent {
                gain: Some(value as f32),
                ..SessionEvent::at(ms, "set_volume", deck)
            },
            EditableLane::EqLow | EditableLane::EqMid | EditableLane::EqHigh => SessionEvent {
                band: Some(self.lane.eq_band().unwrap().to_string()),
                db: Some(value as f32),
                ..SessionEvent::at(ms, "set_eq", deck)
            },
            EditableLane::Filter => SessionEvent {
                value: Some(value as f32),
                ..SessionEvent::at(ms, "set_filter", deck)
            },
            EditableLane::Rate => SessionEvent {
                rate: Some(value),
                ..SessionEvent::at(ms, "set_playback_rate", deck)
            },
            EditableLane::MasterGain => SessionEvent {
                elapsed_ms: ms,
                event_type: "set_master_gain".to_string(),
                gain: Some(value as f32),
                ..Default::default()
            },
        }
    }
}

fn sort_by_ms(mut events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    events.sort_by(crate::sim::event_sim_order);
    events
}

// A drag can scrub back and forth over the same time range; the last value
// written at each timestamp is the one the user ended on.
pub fn normalize_gesture_samples(samples: &[LanePoint]) -> Vec<LanePoint> {
    let mut by_ms: HashMap<u64, f64> = HashMap::new();
    for sample in samples {
        by_ms.insert(sample.ms.to_bits(), sample.value);
    }
    let mut out: Vec<LanePoint> = by_ms
        .into_iter()
        .map(|(bits, value)| LanePoint {
            ms: f64::from_bits(bits),
            value,
        })
        .collect();
    out.sort_by(|first, second| first.ms.partial_cmp(&second.ms).unwrap_or(Ordering::Equal));
    out
}

pub fn decimate_steps(points: &[LanePoint], epsilon: f64) -> Vec<LanePoint> {
    if points.len() <= 1 {
        return points.to_vec();
    }
    let mut out: Vec<LanePoint> = vec![points[0].clone()];
    for point in &points[1..points.len() - 1] {
        if (point.value - out.last().unwrap().value).abs() >= epsilon {
            out.push(point.clone());
        }
    }
    let last = &points[points.len() - 1];
    if last.value != out.last().unwrap().value {
        out.push(last.clone());
    }
    out
}

pub fn original_value_at(
    events: &[SessionEvent],
    spec: &LaneSpec,
    deck: &str,
    ms: f64,
) -> f64 {
    for event in events.iter().rev() {
        if event.elapsed_ms > ms {
            continue;
        }
        if let Some(value) = spec.value_at(event, deck) {
            return value;
        }
    }
    spec.default_value
}

// Replaces this lane's events inside [range_start_ms, range_end_ms] with the
// drawn points and restores the original value at range_end_ms, so everything
// after the gesture sounds unchanged.
pub fn splice_lane_events(
    events: &[SessionEvent],
    spec: &LaneSpec,
    deck: &str,
    range_start_ms: f64,
    range_end_ms: f64,
    points: &[LanePoint],
) -> Vec<SessionEvent> {
    if points.is_empty() || range_end_ms - range_start_ms < MIN_GESTURE_MS {
        return events.to_vec();
    }

    let mut kept: Vec<SessionEvent> = events
        .iter()
        .filter(|event| {
            !(event.elapsed_ms >= range_start_ms
                && event.elapsed_ms <= range_end_ms
                && spec.matches(event, deck))
        })
        .cloned()
        .collect();

    let mut inserted: Vec<SessionEvent> = points
        .iter()
        .map(|point| spec.make_event(point.ms, spec.clamp_value(point.value), deck))
        .collect();

    let restore_value = original_value_at(events, spec, deck, range_end_ms);
    let last_drawn = spec.value_at(inserted.last().unwrap(), deck);
    if last_drawn != Some(restore_value) {
        inserted.push(spec.make_event(range_end_ms, restore_value, deck));
    }

    kept.append(&mut inserted);
    sort_by_ms(kept)
}

// Insert (or replace) a single set_playback_rate point for `deck` at `ms`. The
// rate lane is a step function, so one inserted point holds until the next
// existing change. Backs the editor's "Set BPM" (right-click a clip), which
// converts a target BPM to rate = target_bpm / track_bpm before calling here.
pub fn set_rate_at(
    events: &[SessionEvent],
    deck: &str,
    ms: f64,
    rate: f64,
) -> Vec<SessionEvent> {
    let mut kept: Vec<SessionEvent> = events
        .iter()
        .filter(|event| {
            !(event.event_type == "set_playback_rate"
                && event.deck.as_deref() == Some(deck)
                && (event.elapsed_ms - ms).abs() < f64::EPSILON)
        })
        .cloned()
        .collect();
    kept.push(SessionEvent {
        rate: Some(rate),
        ..SessionEvent::at(ms, "set_playback_rate", deck)
    });
    sort_by_ms(kept)
}

// Set one uniform rate over the whole clip span [start_ms, end_ms]: drop the
// deck's rate changes inside it, open with `rate` at start_ms, and restore the
// pre-edit rate at end_ms so playback after the clip is unchanged. Backs the
// editor's "Set BPM (whole clip)". Unlike splice_lane_events the rate is NOT
// clamped to the lane's display range, since the user typed an explicit BPM.
pub fn set_rate_span(
    events: &[SessionEvent],
    deck: &str,
    start_ms: f64,
    end_ms: f64,
    rate: f64,
) -> Vec<SessionEvent> {
    if end_ms <= start_ms {
        return events.to_vec();
    }
    let spec = lane_spec_for(EditableLane::Rate, None, None);
    let restore = original_value_at(events, &spec, deck, end_ms);
    let mut kept: Vec<SessionEvent> = events
        .iter()
        .filter(|event| {
            !(event.event_type == "set_playback_rate"
                && event.deck.as_deref() == Some(deck)
                && event.elapsed_ms >= start_ms
                && event.elapsed_ms <= end_ms)
        })
        .cloned()
        .collect();
    kept.push(SessionEvent {
        rate: Some(rate),
        ..SessionEvent::at(start_ms, "set_playback_rate", deck)
    });
    if (restore - rate).abs() > f64::EPSILON {
        kept.push(SessionEvent {
            rate: Some(restore),
            ..SessionEvent::at(end_ms, "set_playback_rate", deck)
        });
    }
    sort_by_ms(kept)
}

// Scans backwards for the last event of `event_type` for `deck` at or before
// `ms` (or strictly before, if `!inclusive`) and returns the field `get`
// extracts from it, or `default` if none is found.
fn last_value_at<T: Copy>(
    events: &[SessionEvent],
    deck: &str,
    ms: f64,
    inclusive: bool,
    event_type: &str,
    get: impl Fn(&SessionEvent) -> Option<T>,
    default: T,
) -> T {
    for event in events.iter().rev() {
        let skip = if inclusive {
            event.elapsed_ms > ms
        } else {
            event.elapsed_ms >= ms
        };
        if skip {
            continue;
        }
        if event.event_type == event_type && event.deck.as_deref() == Some(deck) {
            if let Some(value) = get(event) {
                return value;
            }
        }
    }
    default
}

pub fn filter_active_at(
    events: &[SessionEvent],
    deck: &str,
    ms: f64,
    inclusive: bool,
) -> bool {
    last_value_at(
        events,
        deck,
        ms,
        inclusive,
        "set_filter_active",
        |event| event.active,
        false,
    )
}

pub fn nudge_value_at(
    events: &[SessionEvent],
    deck: &str,
    ms: f64,
    inclusive: bool,
) -> f64 {
    last_value_at(
        events,
        deck,
        ms,
        inclusive,
        "set_nudge",
        |event| event.percent,
        0.0,
    )
}

// Replaces every `event_type` event for `deck` in [range_start_ms,
// range_end_ms] with an opener at range_start_ms
// carrying `new_value`, and (if it differs) a restorer at range_end_ms carrying
// `restore_value`. Shared by toggle_filter_active_range and paint_nudge_range.
fn replace_range_with_opener_and_restore<T: PartialEq + Copy>(
    events: &[SessionEvent],
    event_type: &str,
    deck: &str,
    (range_start_ms, range_end_ms): (f64, f64),
    (new_value, restore_value): (T, T),
    set_field: impl Fn(SessionEvent, T) -> SessionEvent,
) -> Vec<SessionEvent> {
    let mut kept: Vec<SessionEvent> = events
        .iter()
        .filter(|event| {
            !(event.event_type == event_type
                && event.deck.as_deref() == Some(deck)
                && event.elapsed_ms >= range_start_ms
                && event.elapsed_ms <= range_end_ms)
        })
        .cloned()
        .collect();

    let mut inserted = vec![set_field(
        SessionEvent::at(range_start_ms, event_type, deck),
        new_value,
    )];
    if restore_value != new_value {
        inserted.push(set_field(
            SessionEvent::at(range_end_ms, event_type, deck),
            restore_value,
        ));
    }

    kept.append(&mut inserted);
    sort_by_ms(kept)
}

// Shift+drag on the filter lane: toggles filter on/off over [range_start_ms,
// range_end_ms], restoring
// the original state at range_end_ms.
pub fn toggle_filter_active_range(
    events: &[SessionEvent],
    deck: &str,
    range_start_ms: f64,
    range_end_ms: f64,
) -> Vec<SessionEvent> {
    if range_end_ms - range_start_ms < MIN_GESTURE_MS {
        return events.to_vec();
    }
    let want = !filter_active_at(events, deck, range_start_ms, false);
    let restore = filter_active_at(events, deck, range_end_ms, true);
    replace_range_with_opener_and_restore(
        events,
        "set_filter_active",
        deck,
        (range_start_ms, range_end_ms),
        (want, restore),
        |event, value| SessionEvent {
            active: Some(value),
            ..event
        },
    )
}

// Shift+drag paints a nudge over [range_start_ms, range_end_ms]; the recorded
// value at range_end_ms is restored.
pub fn paint_nudge_range(
    events: &[SessionEvent],
    deck: &str,
    range_start_ms: f64,
    range_end_ms: f64,
    percent: f64,
) -> Vec<SessionEvent> {
    if range_end_ms - range_start_ms < MIN_GESTURE_MS {
        return events.to_vec();
    }
    let restore = nudge_value_at(events, deck, range_end_ms, true);
    replace_range_with_opener_and_restore(
        events,
        "set_nudge",
        deck,
        (range_start_ms, range_end_ms),
        (percent, restore),
        |event, value| SessionEvent {
            percent: Some(value),
            ..event
        },
    )
}

// Removes every set_nudge for the deck in [range_start_ms, range_end_ms] except a non-zero opener at
// range_end_ms (the adjacent span's start). None = nothing matched (no-op for callers).
pub fn delete_nudge_range(
    events: &[SessionEvent],
    deck: &str,
    range_start_ms: f64,
    range_end_ms: f64,
) -> Option<Vec<SessionEvent>> {
    let in_range = |event: &SessionEvent| {
        event.event_type == "set_nudge"
            && event.deck.as_deref() == Some(deck)
            && event.elapsed_ms >= range_start_ms
            && event.elapsed_ms <= range_end_ms
            && !((event.elapsed_ms - range_end_ms).abs() <= FILTER_SPAN_EPS_MS
                && event.percent != Some(0.0))
    };
    if !events.iter().any(in_range) {
        return None;
    }
    Some(events.iter().filter(|event| !in_range(event)).cloned().collect())
}

const FILTER_SPAN_EPS_MS: f64 = 1.0;

fn is_set_filter_active_event(event: &SessionEvent, deck: &str) -> bool {
    event.event_type == "set_filter_active" && event.deck.as_deref() == Some(deck)
}

// Delete a filter-active span: drop its opening (active=true at start_ms) and,
// if present, its closing (active=false at end_ms). A span that ran to the end
// of the session has no closing event. Filter is off across the gap afterwards.
pub fn delete_filter_active_span(
    events: &[SessionEvent],
    deck: &str,
    start_ms: f64,
    end_ms: f64,
) -> Vec<SessionEvent> {
    events
        .iter()
        .filter(|event| {
            if !is_set_filter_active_event(event, deck) {
                return true;
            }
            let opener = event.active == Some(true)
                && (event.elapsed_ms - start_ms).abs() <= FILTER_SPAN_EPS_MS;
            let closer = event.active == Some(false)
                && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS;
            !(opener || closer)
        })
        .cloned()
        .collect()
}

// Stretch one edge of a filter-active span [start_ms, end_ms] to new_ms, clamped
// so it keeps at least MIN_GESTURE_MS and never crosses the neighbouring filter
// event. Moving the "start" edge relocates the opener; moving "end" relocates the
// closer (or, for a span that ran to the session end, inserts one to close it).
pub fn resize_filter_active_span(
    events: &[SessionEvent],
    deck: &str,
    start_ms: f64,
    end_ms: f64,
    edge: &str,
    new_ms: f64,
    duration_ms: f64,
) -> Vec<SessionEvent> {
    let mut fa_ms: Vec<f64> = events
        .iter()
        .filter(|event| is_set_filter_active_event(event, deck))
        .map(|event| event.elapsed_ms)
        .collect();
    fa_ms.sort_by(|first, second| first.partial_cmp(second).unwrap_or(Ordering::Equal));

    if edge == "start" {
        let prev = fa_ms
            .iter()
            .copied()
            .rfind(|&event_ms| event_ms < start_ms - FILTER_SPAN_EPS_MS)
            .unwrap_or(0.0);
        let max_start_ms = end_ms - MIN_GESTURE_MS;
        if prev > max_start_ms {
            return events.to_vec();
        }
        let clamped = new_ms.clamp(prev, max_start_ms);
        let out: Vec<SessionEvent> = events
            .iter()
            .map(|event| {
                if is_set_filter_active_event(event, deck)
                    && event.active == Some(true)
                    && (event.elapsed_ms - start_ms).abs() <= FILTER_SPAN_EPS_MS
                {
                    SessionEvent {
                        elapsed_ms: clamped,
                        ..event.clone()
                    }
                } else {
                    event.clone()
                }
            })
            .collect();
        return sort_by_ms(out);
    }

    // edge == "end"
    let next = fa_ms
        .iter()
        .copied()
        .find(|&event_ms| event_ms > end_ms + FILTER_SPAN_EPS_MS)
        .unwrap_or(duration_ms);
    let min_end_ms = start_ms + MIN_GESTURE_MS;
    if min_end_ms > next {
        return events.to_vec();
    }
    let clamped = new_ms.clamp(min_end_ms, next);
    let has_closer = events.iter().any(|event| {
        is_set_filter_active_event(event, deck)
            && event.active == Some(false)
            && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS
    });
    if has_closer {
        let out: Vec<SessionEvent> = events
            .iter()
            .map(|event| {
                if is_set_filter_active_event(event, deck)
                    && event.active == Some(false)
                    && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS
                {
                    SessionEvent {
                        elapsed_ms: clamped,
                        ..event.clone()
                    }
                } else {
                    event.clone()
                }
            })
            .collect();
        sort_by_ms(out)
    } else {
        let mut out = events.to_vec();
        out.push(SessionEvent {
            active: Some(false),
            ..SessionEvent::at(clamped, "set_filter_active", deck)
        });
        sort_by_ms(out)
    }
}

// Slide a whole filter-active span [start_ms, end_ms] by delta_ms, shifting its
// opener (active=true) and closer (active=false) together. Clamped so the span
// stays within the gap between its neighbouring filter events and inside
// [0, duration_ms]. A span with no closer (ran to session end) only shifts its
// opener.
pub fn move_filter_active_span(
    events: &[SessionEvent],
    deck: &str,
    start_ms: f64,
    end_ms: f64,
    delta_ms: f64,
    duration_ms: f64,
) -> Vec<SessionEvent> {
    let mut fa_ms: Vec<f64> = events
        .iter()
        .filter(|event| is_set_filter_active_event(event, deck))
        .map(|event| event.elapsed_ms)
        .collect();
    fa_ms.sort_by(|first, second| first.partial_cmp(second).unwrap_or(Ordering::Equal));

    let prev = fa_ms
        .iter()
        .copied()
        .rfind(|&event_ms| event_ms < start_ms - FILTER_SPAN_EPS_MS)
        .unwrap_or(0.0)
        .max(0.0);
    let next = fa_ms
        .iter()
        .copied()
        .find(|&event_ms| event_ms > end_ms + FILTER_SPAN_EPS_MS)
        .unwrap_or(duration_ms)
        .min(duration_ms);

    let min_delta = prev - start_ms;
    let max_delta = next - end_ms;
    if min_delta > max_delta {
        return events.to_vec();
    }
    let delta = delta_ms.clamp(min_delta, max_delta);
    if delta == 0.0 {
        return events.to_vec();
    }

    let out: Vec<SessionEvent> = events
        .iter()
        .map(|event| {
            if is_set_filter_active_event(event, deck) {
                let opener =
                    event.active == Some(true) && (event.elapsed_ms - start_ms).abs() <= FILTER_SPAN_EPS_MS;
                let closer =
                    event.active == Some(false) && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS;
                if opener || closer {
                    return SessionEvent {
                        elapsed_ms: event.elapsed_ms + delta,
                        ..event.clone()
                    };
                }
            }
            event.clone()
        })
        .collect();
    sort_by_ms(out)
}

// Rewrites event track paths after the user relocates missing files.
// None = no event carries a mapped path (no-op for callers).
pub fn relocate_event_paths(
    events: &[SessionEvent],
    mapping: &HashMap<String, String>,
) -> Option<Vec<SessionEvent>> {
    let touches_mapped_path = events.iter().any(|event| {
        event
            .path
            .as_ref()
            .is_some_and(|path| mapping.contains_key(path))
    });
    if !touches_mapped_path {
        return None;
    }
    Some(
        events
            .iter()
            .map(|event| {
                if let Some(path) = &event.path {
                    if let Some(new_path) = mapping.get(path) {
                        let mut out = event.clone();
                        out.path = Some(new_path.clone());
                        return out;
                    }
                }
                event.clone()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent::at(ms, event_type, deck)
    }

    fn lane_point(ms: f64, value: f64) -> LanePoint {
        LanePoint { ms: ms, value }
    }

    fn gain_at(events: &[SessionEvent], ms: f64) -> f64 {
        let spec = lane_spec_for(EditableLane::Gain, None, None);
        original_value_at(events, &spec, "A", ms)
    }

    #[test]
    fn set_rate_at_inserts_one_point_that_holds_until_next_change() {
        let events = vec![
            SessionEvent {
                rate: Some(1.0),
                ..make_event(0.0, "set_playback_rate", "A")
            },
            SessionEvent {
                rate: Some(1.05),
                ..make_event(8000.0, "set_playback_rate", "A")
            },
        ];
        let out = set_rate_at(&events, "A", 3000.0, 0.98);
        let spec = lane_spec_for(EditableLane::Rate, Some(0.92), Some(1.08));
        // Inserted value holds from 3000 until the existing change at 8000.
        assert!((original_value_at(&out, &spec, "A", 3000.0) - 0.98).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 7999.0) - 0.98).abs() < 1e-9);
        // The later change survives (not "to clip end" semantics).
        assert!((original_value_at(&out, &spec, "A", 8000.0) - 1.05).abs() < 1e-9);
        // Earlier value is untouched.
        assert!((original_value_at(&out, &spec, "A", 2999.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn set_rate_span_makes_clip_uniform_and_restores_after() {
        let events = vec![
            SessionEvent {
                rate: Some(1.0),
                ..make_event(0.0, "set_playback_rate", "A")
            },
            // A mid-clip change that "Set BPM (whole clip)" should flatten away.
            SessionEvent {
                rate: Some(1.04),
                ..make_event(5000.0, "set_playback_rate", "A")
            },
        ];
        // Clip span [2000, 8000]; set whole clip to rate 0.97.
        let out = set_rate_span(&events, "A", 2000.0, 8000.0, 0.97);
        let spec = lane_spec_for(EditableLane::Rate, Some(0.92), Some(1.08));
        // Uniform across the clip (the 1.04 mid-clip change is gone).
        assert!((original_value_at(&out, &spec, "A", 2000.0) - 0.97).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 5000.0) - 0.97).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 7999.0) - 0.97).abs() < 1e-9);
        // After the clip the pre-edit rate (1.04, in force at clip end) is restored.
        assert!((original_value_at(&out, &spec, "A", 8000.0) - 1.04).abs() < 1e-9);
        // Before the clip is untouched.
        assert!((original_value_at(&out, &spec, "A", 1000.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn set_rate_at_replaces_an_exact_duplicate() {
        let events = vec![SessionEvent {
            rate: Some(0.95),
            ..make_event(3000.0, "set_playback_rate", "A")
        }];
        let out = set_rate_at(&events, "A", 3000.0, 0.99);
        let rate_events: Vec<_> = out
            .iter()
            .filter(|event| event.event_type == "set_playback_rate")
            .collect();
        assert_eq!(rate_events.len(), 1);
        assert_eq!(rate_events[0].rate, Some(0.99));
    }

    #[test]
    fn splice_gain_applies_inside_range_and_restores_after() {
        let spec = lane_spec_for(EditableLane::Gain, None, None);
        let events = vec![SessionEvent {
            gain: Some(0.8),
            ..make_event(1000.0, "set_volume", "A")
        }];
        let points = vec![lane_point(5000.0, 0.4), lane_point(6000.0, 0.4)];
        let out = splice_lane_events(&events, &spec, "A", 5000.0, 8000.0, &points);
        // before the gesture: original 0.8
        assert!((gain_at(&out, 3000.0) - 0.8).abs() < 1e-6);
        // inside: 0.4
        assert!((gain_at(&out, 5500.0) - 0.4).abs() < 1e-6);
        // restored to 0.8 at range_end_ms
        assert!((gain_at(&out, 9000.0) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn splice_rejects_too_short_gesture() {
        let spec = lane_spec_for(EditableLane::Gain, None, None);
        let events = vec![make_event(0.0, "set_volume", "A")];
        let out = splice_lane_events(&events, &spec, "A", 1000.0, 1020.0, &[lane_point(1000.0, 0.5)]);
        assert_eq!(out.len(), events.len());
    }

    #[test]
    fn filter_dead_zone_snaps_to_zero() {
        let spec = lane_spec_for(EditableLane::Filter, None, None);
        assert_eq!(spec.clamp_value(0.03), 0.0);
        assert!((spec.clamp_value(0.5) - 0.5).abs() < 1e-9);
        assert_eq!(spec.clamp_value(2.0), 1.0); // clamped to max then not in dead zone
    }

    #[test]
    fn toggle_filter_active_pairs_into_span() {
        let out = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        // was off -> turns on at range_start_ms, restores to off at range_end_ms
        assert!(filter_active_at(&out, "A", 2000.0, true));
        assert!(!filter_active_at(&out, "A", 5000.0, true));
    }

    #[test]
    fn delete_filter_active_span_removes_on_off_pair() {
        let events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        assert_eq!(events.len(), 2);
        let out = delete_filter_active_span(&events, "A", 1000.0, 4000.0);
        assert!(out.is_empty());
        assert!(!filter_active_at(&out, "A", 2000.0, true));
    }

    #[test]
    fn resize_filter_active_span_moves_end_edge() {
        let events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        let out = resize_filter_active_span(&events, "A", 1000.0, 4000.0, "end", 6000.0, 10_000.0);
        assert!(filter_active_at(&out, "A", 5000.0, true)); // span now extends past old end
        assert!(!filter_active_at(&out, "A", 7000.0, true));
    }

    #[test]
    fn resize_filter_active_span_start_clamps_to_min_gesture() {
        let events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        // Dragging the start past the end is clamped to keep at least MIN_GESTURE_MS.
        let out = resize_filter_active_span(&events, "A", 1000.0, 4000.0, "start", 9000.0, 10_000.0);
        let opener = out
            .iter()
            .find(|event| event.event_type == "set_filter_active" && event.active == Some(true))
            .unwrap();
        assert!(opener.elapsed_ms <= 4000.0 - MIN_GESTURE_MS + 1e-6);
    }

    #[test]
    fn resize_open_filter_span_inserts_closer() {
        // A span with no closing event (ran to session end) gets one when its
        // end edge is dragged in.
        let events = vec![SessionEvent {
            active: Some(true),
            ..make_event(1000.0, "set_filter_active", "A")
        }];
        let out = resize_filter_active_span(&events, "A", 1000.0, 10_000.0, "end", 5000.0, 10_000.0);
        assert!(filter_active_at(&out, "A", 3000.0, true));
        assert!(!filter_active_at(&out, "A", 6000.0, true));
    }

    #[test]
    fn move_filter_active_span_slides_both_edges() {
        let mut events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        // The drawn cutoff curve (set_filter) must NOT move with the span.
        events.push(make_event(2000.0, "set_filter", "A"));
        let out = move_filter_active_span(&events, "A", 1000.0, 4000.0, 2000.0, 10_000.0);
        assert!(!filter_active_at(&out, "A", 2000.0, true)); // old start now off
        assert!(filter_active_at(&out, "A", 5000.0, true)); // shifted +2000
        assert!(!filter_active_at(&out, "A", 6500.0, true)); // new end at 6000
        let cutoff = out
            .iter()
            .find(|event| event.event_type == "set_filter")
            .unwrap();
        assert!((cutoff.elapsed_ms - 2000.0).abs() < 1e-6); // value point stayed put
    }

    #[test]
    fn move_filter_active_span_clamps_to_session_end() {
        let events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        // Dragging far right is clamped so the end never passes duration_ms.
        let out = move_filter_active_span(&events, "A", 1000.0, 4000.0, 100_000.0, 10_000.0);
        let close = out
            .iter()
            .find(|event| event.event_type == "set_filter_active" && event.active == Some(false))
            .unwrap();
        assert!(close.elapsed_ms <= 10_000.0 + 1e-6);
    }

    #[test]
    fn paint_nudge_range_inserts_open_and_restore() {
        let out = paint_nudge_range(&[], "A", 1000.0, 1500.0, 8.0);
        assert!((nudge_value_at(&out, "A", 1200.0, true) - 8.0).abs() < 1e-9);
        assert_eq!(nudge_value_at(&out, "A", 2000.0, true), 0.0);
    }

    #[test]
    fn delete_nudge_range_keeps_adjacent_opener_at_t1() {
        let events = vec![
            SessionEvent {
                percent: Some(5.0),
                ..make_event(1000.0, "set_nudge", "A")
            },
            SessionEvent {
                percent: Some(0.0),
                ..make_event(2000.0, "set_nudge", "A")
            },
            SessionEvent {
                percent: Some(-5.0),
                ..make_event(2000.0, "set_nudge", "A")
            },
        ];
        // Delete [1000, 2000]: removes the opener at 1000 and the closing zero at
        // 2000, but keeps the non-zero opener at 2000.
        let out = delete_nudge_range(&events, "A", 1000.0, 2000.0).unwrap();
        let remaining: Vec<f64> = out
            .iter()
            .filter(|event| event.event_type == "set_nudge")
            .map(|event| event.percent.unwrap())
            .collect();
        assert_eq!(remaining, vec![-5.0]);
    }

    #[test]
    fn delete_nudge_range_noop_when_nothing_matches() {
        let events = vec![make_event(0.0, "set_volume", "A")];
        assert!(delete_nudge_range(&events, "A", 1000.0, 2000.0).is_none());
    }

    #[test]
    fn relocate_event_paths_rewrites_mapped_paths() {
        let events = vec![
            SessionEvent {
                path: Some("/old/a.mp3".to_string()),
                ..make_event(0.0, "load_track", "A")
            },
            SessionEvent {
                path: Some("/keep/b.mp3".to_string()),
                ..make_event(1.0, "load_track", "B")
            },
        ];
        let mut mapping = HashMap::new();
        mapping.insert("/old/a.mp3".to_string(), "/new/a.mp3".to_string());
        let out = relocate_event_paths(&events, &mapping).unwrap();
        assert_eq!(out[0].path.as_deref(), Some("/new/a.mp3"));
        assert_eq!(out[1].path.as_deref(), Some("/keep/b.mp3"));

        let mut unrelated = HashMap::new();
        unrelated.insert("/never/there.mp3".to_string(), "/new/c.mp3".to_string());
        assert!(relocate_event_paths(&events, &unrelated).is_none());
    }

    #[test]
    fn normalize_gesture_keeps_last_value_per_ms_and_sorts() {
        let samples = vec![lane_point(200.0, 0.1), lane_point(100.0, 0.9), lane_point(200.0, 0.5)];
        let out = normalize_gesture_samples(&samples);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ms, 100.0);
        assert_eq!(out[1].ms, 200.0);
        assert_eq!(out[1].value, 0.5);
    }

    #[test]
    fn decimate_drops_steps_below_epsilon() {
        let points = vec![lane_point(0.0, 0.0), lane_point(1.0, 0.005), lane_point(2.0, 0.5)];
        let out = decimate_steps(&points, 0.01);
        // middle point (delta 0.005 < eps) dropped; first + last kept.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].value, 0.0);
        assert_eq!(out[1].value, 0.5);
    }

    #[test]
    fn rate_lane_reads_snapshot_and_uses_supplied_range() {
        let spec = lane_spec_for(EditableLane::Rate, Some(0.9), Some(1.1));
        assert_eq!(spec.min, 0.9);
        assert_eq!(spec.max, 1.1);
        let events = vec![SessionEvent {
            playback_rate: Some(0.97),
            ..make_event(0.0, "deck_snapshot", "A")
        }];
        assert!((original_value_at(&events, &spec, "A", 1000.0) - 0.97).abs() < 1e-6);
    }
}
