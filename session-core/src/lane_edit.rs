use crate::event::SessionEvent;
use crate::param::{MixerManifest, ParamDescriptor, ParamScope};
use crate::timeline::{LanePoint, DEFAULT_RATE};
use std::cmp::Ordering;
use std::collections::HashMap;

// Gestures spanning less time than this are rejected: the value would change and
// restore almost instantly, inaudible and rendering as a bare vertical line.
pub const MIN_GESTURE_MS: f64 = 50.0;

// Public: the native DSP and the frontend mixer read these from here.
pub const EQ_MIN_DB: f64 = -26.0;
pub const EQ_MAX_DB: f64 = 6.0;
pub const FILTER_DEAD_ZONE: f64 = 0.05;

// Fallback range when no clip-specific one is given. Rate is transport, so it
// has no descriptor to read these from.
const RATE_MIN: f64 = 0.92;
const RATE_MAX: f64 = 1.08;
const RATE_UNIT: &str = "ratio";
const NUDGE_UNIT: &str = "percent";

// What a hand can usefully draw. A wheel spike can exceed it, and the lane clips
// it: a range wide enough for the worst recorded gesture leaves a nudge invisible.
const NUDGE_MIN_PCT: f64 = -16.0;
const NUDGE_MAX_PCT: f64 = 16.0;

/// How the editor labels a lane. `unit` is what the value means, so a gesture
/// readout cannot print dB for a mixer whose eq is a 0-1 kill.
pub struct LaneDisplay {
    pub unit: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditableLane {
    Gain,
    EqLow,
    EqMid,
    EqHigh,
    Filter,
    Rate,
    Jog,
    MasterGain,
    Xfader,
}

impl EditableLane {
    pub const ALL: [EditableLane; 9] = [
        Self::Gain,
        Self::Filter,
        Self::Rate,
        Self::Jog,
        Self::EqLow,
        Self::EqMid,
        Self::EqHigh,
        Self::MasterGain,
        Self::Xfader,
    ];

    pub fn key(&self) -> &'static str {
        match self {
            Self::Gain => "gain",
            Self::EqLow => "eqLow",
            Self::EqMid => "eqMid",
            Self::EqHigh => "eqHigh",
            Self::Filter => "filter",
            Self::Rate => "rate",
            Self::Jog => "jog",
            Self::MasterGain => "masterGain",
            Self::Xfader => "xfader",
        }
    }

    pub fn from_key(key: &str) -> Option<EditableLane> {
        Self::ALL.into_iter().find(|lane| lane.key() == key)
    }

    // None for the transport lanes, which are not mixer params.
    pub fn slot_param(&self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::Gain => ("fader", "gain"),
            Self::EqLow => ("eq", "low"),
            Self::EqMid => ("eq", "mid"),
            Self::EqHigh => ("eq", "high"),
            Self::Filter => ("filter", "value"),
            Self::MasterGain => ("gain", "gain"),
            Self::Xfader => ("xfader", "position"),
            Self::Rate | Self::Jog => return None,
        })
    }

    pub fn scope(&self) -> ParamScope {
        match self {
            Self::MasterGain | Self::Xfader => ParamScope::Master,
            _ => ParamScope::Deck,
        }
    }

    /// None for Rate, and for a lane the given mixer does not have: an eq lane
    /// is only editable on a mixer that has an eq slot.
    pub fn descriptor(&self, mixer: &'static MixerManifest) -> Option<&'static ParamDescriptor> {
        let (slot, param) = self.slot_param()?;
        mixer.descriptor(self.scope(), slot, param)
    }

    /// Lets the frozen v1 manifests, which have no crossfader slot, still draw the lane.
    fn canonical_descriptor(&self) -> Option<&'static ParamDescriptor> {
        match self {
            Self::Xfader => Some(crate::param::xfader_position_descriptor()),
            _ => None,
        }
    }

    pub fn display(&self, mixer: &'static MixerManifest) -> LaneDisplay {
        match self
            .descriptor(mixer)
            .or_else(|| self.canonical_descriptor())
        {
            Some(descriptor) => LaneDisplay {
                unit: descriptor.unit.id(),
            },
            None => LaneDisplay {
                unit: match self {
                    Self::Jog => NUDGE_UNIT,
                    _ => RATE_UNIT,
                },
            },
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

// Epsilon stays here rather than on the descriptor: it is the gesture
// decimation tolerance for the editor, not a property of the audio parameter.
fn lane_epsilon(lane: EditableLane) -> f64 {
    match lane {
        EditableLane::EqLow | EditableLane::EqMid | EditableLane::EqHigh => 0.25,
        EditableLane::Rate => 0.0005,
        EditableLane::Jog => 0.1,
        _ => 0.01,
    }
}

pub fn rate_lane_spec(rate_min: Option<f64>, rate_max: Option<f64>) -> LaneSpec {
    LaneSpec {
        lane: EditableLane::Rate,
        min: rate_min.unwrap_or(RATE_MIN),
        max: rate_max.unwrap_or(RATE_MAX),
        default_value: DEFAULT_RATE,
        epsilon: lane_epsilon(EditableLane::Rate),
    }
}

pub fn jog_lane_spec() -> LaneSpec {
    LaneSpec {
        lane: EditableLane::Jog,
        min: NUDGE_MIN_PCT,
        max: NUDGE_MAX_PCT,
        default_value: 0.0,
        epsilon: lane_epsilon(EditableLane::Jog),
    }
}

pub fn lane_spec_for(
    lane: EditableLane,
    mixer: &'static MixerManifest,
    rate_min: Option<f64>,
    rate_max: Option<f64>,
) -> LaneSpec {
    if lane == EditableLane::Jog {
        return jog_lane_spec();
    }
    // Rate is the only remaining lane without a descriptor of its own.
    // `every_mixer_lane_resolves_to_a_descriptor` stops a mixer lane landing here.
    let Some(descriptor) = lane
        .descriptor(mixer)
        .or_else(|| lane.canonical_descriptor())
    else {
        return rate_lane_spec(rate_min, rate_max);
    };
    LaneSpec {
        lane,
        min: descriptor.min,
        max: descriptor.max,
        default_value: descriptor.default,
        epsilon: lane_epsilon(lane),
    }
}

impl LaneSpec {
    // The decimation tolerance, not the filter's dead zone: a knob nudged a
    // little is inaudible but still a recorded move a reset should reach.
    pub fn is_default(&self, value: f64) -> bool {
        (value - self.default_value).abs() <= self.epsilon
    }

    pub fn clamp_value(&self, value: f64) -> f64 {
        self.max.min(self.min.max(value))
    }

    fn event_deck<'a>(&self, deck: &'a str) -> Option<&'a str> {
        match self.lane.scope() {
            ParamScope::Master => None,
            ParamScope::Deck => Some(deck),
        }
    }

    fn matches(&self, event: &SessionEvent, deck: &str) -> bool {
        match self.lane {
            // Both, because the lane plots one summed deviation: a drawn curve
            // replaces the wheel gesture rather than having to cancel it.
            EditableLane::Jog => {
                matches!(event.event_type.as_str(), "set_nudge" | "jog")
                    && event.deck.as_deref() == Some(deck)
            }
            EditableLane::Rate => {
                event.event_type == "set_playback_rate" && event.deck.as_deref() == Some(deck)
            }
            _ => match self.lane.slot_param() {
                Some((slot, param)) => event.is_param(self.event_deck(deck), slot, param),
                None => false,
            },
        }
    }

    fn value_at(&self, event: &SessionEvent, deck: &str) -> Option<f64> {
        if self.lane == EditableLane::Jog {
            return self.matches(event, deck).then_some(event.percent)?;
        }
        if self.lane == EditableLane::Rate {
            let deck_ok = event.deck.as_deref() == Some(deck);
            if event.event_type == "set_playback_rate" && deck_ok {
                return event.rate;
            }
            if event.event_type == "deck_snapshot" && deck_ok {
                return event.playback_rate;
            }
            return None;
        }
        self.matches(event, deck)
            .then(|| event.value.map(|value| value as f64))?
    }

    fn make_event(&self, ms: f64, value: f64, deck: &str) -> SessionEvent {
        if self.lane == EditableLane::Jog {
            return SessionEvent {
                percent: Some(value),
                ..SessionEvent::at(ms, "set_nudge", deck)
            };
        }
        if self.lane == EditableLane::Rate {
            return SessionEvent {
                rate: Some(value),
                ..SessionEvent::at(ms, "set_playback_rate", deck)
            };
        }
        let (slot, param) = self
            .lane
            .slot_param()
            .expect("every non-rate lane addresses a mixer param");
        SessionEvent::param(ms, self.event_deck(deck), slot, param, value)
    }
}

// A drag can scrub back and forth over the same time range. The last value
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

pub fn original_value_at(events: &[SessionEvent], spec: &LaneSpec, deck: &str, ms: f64) -> f64 {
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

    // Sorted first because `points` arrives from a public API and a drag can scrub
    // backwards. The restore check has to compare against the temporally last value.
    let mut inserted: Vec<SessionEvent> = crate::sim::sorted_by_sim_order(
        points
            .iter()
            .map(|point| spec.make_event(point.ms, spec.clamp_value(point.value), deck))
            .collect(),
    );

    let restore_value = original_value_at(events, spec, deck, range_end_ms);
    let last_drawn = spec.value_at(inserted.last().unwrap(), deck);
    if last_drawn != Some(restore_value) {
        inserted.push(spec.make_event(range_end_ms, restore_value, deck));
    }

    kept.append(&mut inserted);
    crate::sim::sorted_by_sim_order(kept)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetExtent {
    ToEnd,
    UntilHere,
    ThisMove,
}

// `ThisMove` covers the excursion the click landed in: back to where the curve
// left the default and forward to where it reaches it again. Stopping at the
// neighbouring events instead would flatten a millisecond, since a recorded
// curve changes on every frame it was moved.
// The excursion around `ms`: from where the curve last came to rest at the
// default to where it rests there again, or None where the lane is at rest.
pub fn lane_move_span(
    events: &[SessionEvent],
    spec: &LaneSpec,
    deck: &str,
    ms: f64,
) -> Option<(f64, f64)> {
    if spec.is_default(original_value_at(events, spec, deck, ms)) {
        return None;
    }
    let lane = lane_values(events, spec, deck);
    let runs = side_runs(&lane, spec);
    let here = runs
        .iter()
        .rposition(|run| lane[run.0].0 <= ms && run.2 != 0)
        .unwrap_or(0);

    // The run's own events, inclusive: bounding it by the event after it would
    // miss the last one whenever the crossing was recorded on the same
    // millisecond, and the move on the other side begins at its own first event.
    Some((lane[runs[here].0].0.min(ms), lane[runs[here].1].0))
}

// (first index, last index, side) for each run of values on one side of the
// default: above it, below it, or on it.
fn side_runs(lane: &[(f64, f64)], spec: &LaneSpec) -> Vec<(usize, usize, i8)> {
    let mut runs: Vec<(usize, usize, i8)> = Vec::new();
    for (idx, (_, value)) in lane.iter().enumerate() {
        let side = if spec.is_default(*value) {
            0
        } else if *value > spec.default_value {
            1
        } else {
            -1
        };
        match runs.last_mut() {
            Some(run) if run.2 == side => run.1 = idx,
            _ => runs.push((idx, idx, side)),
        }
    }
    runs
}

pub fn reset_lane_from(
    events: &[SessionEvent],
    spec: &LaneSpec,
    deck: &str,
    ms: f64,
    extent: ResetExtent,
) -> Vec<SessionEvent> {
    let span = lane_move_span(events, spec, deck, ms);
    if extent == ResetExtent::ThisMove && span.is_none() {
        return events.to_vec();
    }
    let (move_start, move_end) = span.unwrap_or((ms, ms));

    let drops = |at: f64| match extent {
        ResetExtent::ToEnd => at >= ms,
        ResetExtent::UntilHere => at <= ms,
        ResetExtent::ThisMove => at >= move_start && at <= move_end,
    };

    let mut kept: Vec<SessionEvent> = events
        .iter()
        .filter(|event| !(spec.matches(event, deck) && drops(event.elapsed_ms)))
        .cloned()
        .collect();
    // Nothing to hold before the first event: the lane starts at its default.
    if extent != ResetExtent::UntilHere {
        let at = if extent == ResetExtent::ThisMove {
            move_start
        } else {
            ms
        };
        if !spec.is_default(original_value_at(&kept, spec, deck, at)) {
            kept.push(spec.make_event(at, spec.default_value, deck));
        }
    }
    crate::sim::sorted_by_sim_order(kept)
}

fn lane_values(events: &[SessionEvent], spec: &LaneSpec, deck: &str) -> Vec<(f64, f64)> {
    let mut values: Vec<(f64, f64)> = events
        .iter()
        .filter(|event| spec.matches(event, deck))
        .filter_map(|event| {
            spec.value_at(event, deck)
                .map(|value| (event.elapsed_ms, value))
        })
        .collect();
    values.sort_by(|first, second| first.0.partial_cmp(&second.0).unwrap_or(Ordering::Equal));
    values
}

// The rate lane is a step function, so one inserted point holds until the next change.
pub fn set_rate_at(events: &[SessionEvent], deck: &str, ms: f64, rate: f64) -> Vec<SessionEvent> {
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
    crate::sim::sorted_by_sim_order(kept)
}

// The rate is not clamped to the lane's display range: the user typed a BPM.
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
    let spec = rate_lane_spec(None, None);
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
    crate::sim::sorted_by_sim_order(kept)
}

fn last_value_at<T: Copy>(
    events: &[SessionEvent],
    ms: f64,
    inclusive: bool,
    matches: impl Fn(&SessionEvent) -> bool,
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
        if matches(event) {
            if let Some(value) = get(event) {
                return value;
            }
        }
    }
    default
}

pub fn filter_active_at(events: &[SessionEvent], deck: &str, ms: f64, inclusive: bool) -> bool {
    last_value_at(
        events,
        ms,
        inclusive,
        |event| event.is_param(Some(deck), "filter", "active"),
        |event| event.value.map(|value| value != 0.0),
        false,
    )
}

fn replace_range_with_opener_and_restore<T: PartialEq + Copy>(
    events: &[SessionEvent],
    matches: impl Fn(&SessionEvent) -> bool,
    make: impl Fn(f64, T) -> SessionEvent,
    (range_start_ms, range_end_ms): (f64, f64),
    (new_value, restore_value): (T, T),
) -> Vec<SessionEvent> {
    let mut kept: Vec<SessionEvent> = events
        .iter()
        .filter(|event| {
            !(matches(event)
                && event.elapsed_ms >= range_start_ms
                && event.elapsed_ms <= range_end_ms)
        })
        .cloned()
        .collect();

    let mut inserted = vec![make(range_start_ms, new_value)];
    if restore_value != new_value {
        inserted.push(make(range_end_ms, restore_value));
    }

    kept.append(&mut inserted);
    crate::sim::sorted_by_sim_order(kept)
}

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
        |event| event.is_param(Some(deck), "filter", "active"),
        |ms, value: bool| {
            SessionEvent::param(
                ms,
                Some(deck),
                "filter",
                "active",
                if value { 1.0 } else { 0.0 },
            )
        },
        (range_start_ms, range_end_ms),
        (want, restore),
    )
}

const FILTER_SPAN_EPS_MS: f64 = 1.0;

fn is_set_filter_active_event(event: &SessionEvent, deck: &str) -> bool {
    event.is_param(Some(deck), "filter", "active")
}

// A span that ran to the end of the session has no closing event to drop.
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
            let opener = event.value == Some(1.0)
                && (event.elapsed_ms - start_ms).abs() <= FILTER_SPAN_EPS_MS;
            let closer =
                event.value == Some(0.0) && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS;
            !(opener || closer)
        })
        .cloned()
        .collect()
}

// A span that ran to the session end has no closer, so moving that edge in
// inserts one rather than relocating it.
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
                    && event.value == Some(1.0)
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
        return crate::sim::sorted_by_sim_order(out);
    }

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
            && event.value == Some(0.0)
            && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS
    });
    if has_closer {
        let out: Vec<SessionEvent> = events
            .iter()
            .map(|event| {
                if is_set_filter_active_event(event, deck)
                    && event.value == Some(0.0)
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
        crate::sim::sorted_by_sim_order(out)
    } else {
        let mut out = events.to_vec();
        out.push(SessionEvent::param(
            clamped,
            Some(deck),
            "filter",
            "active",
            0.0,
        ));
        crate::sim::sorted_by_sim_order(out)
    }
}

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
                let opener = event.value == Some(1.0)
                    && (event.elapsed_ms - start_ms).abs() <= FILTER_SPAN_EPS_MS;
                let closer = event.value == Some(0.0)
                    && (event.elapsed_ms - end_ms).abs() <= FILTER_SPAN_EPS_MS;
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
    crate::sim::sorted_by_sim_order(out)
}

// None = no event carries a mapped path, so callers can keep their input.
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
    use crate::param::CLASSIC_3BAND;

    #[test]
    fn all_lanes_round_trip_through_their_key() {
        let mut seen = Vec::new();
        for lane in EditableLane::ALL {
            let key = lane.key();
            assert_eq!(
                EditableLane::from_key(key),
                Some(lane),
                "round trip for {key}"
            );
            assert!(!seen.contains(&key), "duplicate lane key {key}");
            seen.push(key);
        }
        assert_eq!(seen.len(), EditableLane::ALL.len());
        assert_eq!(EditableLane::from_key("nope"), None);
    }

    fn make_event(ms: f64, event_type: &str, deck: &str) -> SessionEvent {
        SessionEvent::at(ms, event_type, deck)
    }

    fn lane_point(ms: f64, value: f64) -> LanePoint {
        LanePoint { ms: ms, value }
    }

    fn gain_at(events: &[SessionEvent], ms: f64) -> f64 {
        let spec = lane_spec_for(EditableLane::Gain, &CLASSIC_3BAND, None, None);
        original_value_at(events, &spec, "A", ms)
    }

    #[test]
    fn splice_restores_value_when_points_are_unordered() {
        let spec = lane_spec_for(EditableLane::Gain, &CLASSIC_3BAND, None, None);
        let events = vec![spec.make_event(0.0, 0.7, "A")];
        // Temporally last point is (500, 0.97). Last in input order is (100, 0.7),
        // which equals the value to restore.
        let points = vec![
            LanePoint {
                ms: 500.0,
                value: 0.97,
            },
            LanePoint {
                ms: 100.0,
                value: 0.7,
            },
        ];
        let after = splice_lane_events(&events, &spec, "A", 0.0, 1000.0, &points);
        // The gain lane stores f32, so compare with tolerance rather than exactly.
        let restored = original_value_at(&after, &spec, "A", 1001.0);
        assert!(
            (restored - 0.7).abs() < 1e-6,
            "value after the spliced range must be restored, got {restored}"
        );
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
        let spec = rate_lane_spec(Some(0.92), Some(1.08));
        assert!((original_value_at(&out, &spec, "A", 3000.0) - 0.98).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 7999.0) - 0.98).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 8000.0) - 1.05).abs() < 1e-9);
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
        let out = set_rate_span(&events, "A", 2000.0, 8000.0, 0.97);
        let spec = rate_lane_spec(Some(0.92), Some(1.08));
        assert!((original_value_at(&out, &spec, "A", 2000.0) - 0.97).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 5000.0) - 0.97).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 7999.0) - 0.97).abs() < 1e-9);
        assert!((original_value_at(&out, &spec, "A", 8000.0) - 1.04).abs() < 1e-9);
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
        let spec = lane_spec_for(EditableLane::Gain, &CLASSIC_3BAND, None, None);
        let events = vec![SessionEvent::param(1000.0, Some("A"), "fader", "gain", 0.8)];
        let points = vec![lane_point(5000.0, 0.4), lane_point(6000.0, 0.4)];
        let out = splice_lane_events(&events, &spec, "A", 5000.0, 8000.0, &points);
        assert!((gain_at(&out, 3000.0) - 0.8).abs() < 1e-6);
        assert!((gain_at(&out, 5500.0) - 0.4).abs() < 1e-6);
        assert!((gain_at(&out, 9000.0) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn splice_rejects_too_short_gesture() {
        let spec = lane_spec_for(EditableLane::Gain, &CLASSIC_3BAND, None, None);
        let events = vec![SessionEvent::param(0.0, Some("A"), "fader", "gain", 1.0)];
        let out = splice_lane_events(
            &events,
            &spec,
            "A",
            1000.0,
            1020.0,
            &[lane_point(1000.0, 0.5)],
        );
        assert_eq!(out.len(), events.len());
    }

    #[test]
    fn a_drawn_filter_value_keeps_its_shape_through_the_dead_zone() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        assert!((spec.clamp_value(0.03) - 0.03).abs() < 1e-9);
        assert!((spec.clamp_value(0.5) - 0.5).abs() < 1e-9);
        assert_eq!(spec.clamp_value(2.0), 1.0);
    }

    #[test]
    fn reset_lane_from_returns_the_lane_to_its_default() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            4000.0,
            &[lane_point(1000.0, 0.5), lane_point(4000.0, 0.5)],
        );

        let out = reset_lane_from(&events, &spec, "A", 3000.0, ResetExtent::ToEnd);

        assert_eq!(original_value_at(&out, &spec, "A", 2000.0), 0.5);
        assert_eq!(
            original_value_at(&out, &spec, "A", 3000.0),
            spec.default_value
        );
        assert_eq!(
            original_value_at(&out, &spec, "A", 9000.0),
            spec.default_value
        );
    }

    #[test]
    fn reset_lane_to_end_wipes_later_moves_even_from_a_flat_stretch() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.5),
                lane_point(3000.0, 0.0),
                lane_point(6000.0, 0.9),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 4000.0, ResetExtent::ToEnd);

        assert!((original_value_at(&out, &spec, "A", 2000.0) - 0.5).abs() < 1e-6);
        assert!(spec.is_default(original_value_at(&out, &spec, "A", 7000.0)));
    }

    #[test]
    fn reset_lane_until_here_wipes_earlier_moves_and_keeps_the_rest() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.5),
                lane_point(3000.0, 0.8),
                lane_point(6000.0, 0.9),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 4000.0, ResetExtent::UntilHere);

        assert!(spec.is_default(original_value_at(&out, &spec, "A", 2000.0)));
        assert!(spec.is_default(original_value_at(&out, &spec, "A", 4000.0)));
        assert!((original_value_at(&out, &spec, "A", 7000.0) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn reset_lane_this_move_does_nothing_where_the_lane_is_already_at_its_default() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            4000.0,
            &[lane_point(1000.0, 0.5), lane_point(4000.0, 0.5)],
        );

        let out = reset_lane_from(&events, &spec, "A", 9000.0, ResetExtent::ThisMove);

        assert_eq!(out.len(), events.len());
    }

    #[test]
    fn reset_lane_this_move_flattens_the_whole_excursion_around_the_click() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.0),
                lane_point(2000.0, 0.5),
                lane_point(4000.0, 0.8),
                lane_point(6000.0, 0.0),
                lane_point(7000.0, 0.7),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 4500.0, ResetExtent::ThisMove);

        assert!(spec.is_default(original_value_at(&out, &spec, "A", 2500.0)));
        assert!(spec.is_default(original_value_at(&out, &spec, "A", 5000.0)));
        assert!((original_value_at(&out, &spec, "A", 7500.0) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn reset_lane_this_move_reaches_a_move_too_small_to_be_audible() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.0),
                lane_point(2000.0, -0.03),
                lane_point(3000.0, 0.0),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 2500.0, ResetExtent::ThisMove);

        assert!(spec.is_default(original_value_at(&out, &spec, "A", 2500.0)));
    }

    #[test]
    fn reset_lane_this_move_takes_a_drop_recorded_on_the_same_millisecond() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = vec![
            SessionEvent::param(1000.0, Some("A"), "filter", "value", 0.3),
            SessionEvent::param(4000.0, Some("A"), "filter", "value", 0.9),
            SessionEvent::param(4000.0, Some("A"), "filter", "value", 0.0),
        ];

        let out = reset_lane_from(&events, &spec, "A", 2000.0, ResetExtent::ThisMove);

        assert!(spec.is_default(original_value_at(&out, &spec, "A", 4000.0)));
        assert!(spec.is_default(original_value_at(&out, &spec, "A", 9000.0)));
    }

    #[test]
    fn reset_lane_this_move_stops_at_a_crossing_no_event_landed_on() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.0),
                lane_point(2000.0, 0.8),
                lane_point(4000.0, 0.02),
                lane_point(4100.0, -0.6),
                lane_point(6000.0, -0.9),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 3000.0, ResetExtent::ThisMove);

        assert!(spec.is_default(original_value_at(&out, &spec, "A", 3000.0)));
        assert!((original_value_at(&out, &spec, "A", 5000.0) + 0.6).abs() < 1e-6);
        assert!((original_value_at(&out, &spec, "A", 7000.0) + 0.9).abs() < 1e-6);
    }

    #[test]
    fn reset_lane_this_move_stops_where_the_curve_crosses_the_default() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.0),
                lane_point(1500.0, -0.4),
                lane_point(1600.0, 0.0),
                lane_point(1700.0, 0.5),
                lane_point(4000.0, 0.9),
                lane_point(6000.0, 0.0),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 3000.0, ResetExtent::ThisMove);

        assert!((original_value_at(&out, &spec, "A", 1550.0) + 0.4).abs() < 1e-6);
        assert!(spec.is_default(original_value_at(&out, &spec, "A", 4000.0)));
    }

    #[test]
    fn reset_lane_this_move_leaves_a_later_excursion_alone() {
        let spec = lane_spec_for(EditableLane::Filter, &CLASSIC_3BAND, None, None);
        let events = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            9000.0,
            &[
                lane_point(1000.0, 0.0),
                lane_point(2000.0, -0.5),
                lane_point(3000.0, 0.0),
                lane_point(5000.0, -0.9),
            ],
        );

        let out = reset_lane_from(&events, &spec, "A", 2500.0, ResetExtent::ThisMove);

        assert!(spec.is_default(original_value_at(&out, &spec, "A", 2500.0)));
        assert!((original_value_at(&out, &spec, "A", 6000.0) + 0.9).abs() < 1e-6);
    }

    #[test]
    fn toggle_filter_active_pairs_into_span() {
        let out = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
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
        let out =
            resize_filter_active_span(&events, "A", 1000.0, 4000.0, "start", 9000.0, 10_000.0);
        let opener = out
            .iter()
            .find(|event| event.is_param(Some("A"), "filter", "active") && event.value == Some(1.0))
            .unwrap();
        assert!(opener.elapsed_ms <= 4000.0 - MIN_GESTURE_MS + 1e-6);
    }

    #[test]
    fn resize_open_filter_span_inserts_closer() {
        // A span with no closing event (ran to session end) gets one when its
        // end edge is dragged in.
        let events = vec![SessionEvent::param(
            1000.0,
            Some("A"),
            "filter",
            "active",
            1.0,
        )];
        let out =
            resize_filter_active_span(&events, "A", 1000.0, 10_000.0, "end", 5000.0, 10_000.0);
        assert!(filter_active_at(&out, "A", 3000.0, true));
        assert!(!filter_active_at(&out, "A", 6000.0, true));
    }

    #[test]
    fn move_filter_active_span_slides_both_edges() {
        let mut events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        events.push(SessionEvent::param(
            2000.0,
            Some("A"),
            "filter",
            "value",
            0.5,
        ));
        let out = move_filter_active_span(&events, "A", 1000.0, 4000.0, 2000.0, 10_000.0);
        assert!(!filter_active_at(&out, "A", 2000.0, true)); // old start now off
        assert!(filter_active_at(&out, "A", 5000.0, true)); // shifted +2000
        assert!(!filter_active_at(&out, "A", 6500.0, true)); // new end at 6000
        let cutoff = out
            .iter()
            .find(|event| event.is_param(Some("A"), "filter", "value"))
            .unwrap();
        assert!((cutoff.elapsed_ms - 2000.0).abs() < 1e-6); // value point stayed put
    }

    #[test]
    fn move_filter_active_span_clamps_to_session_end() {
        let events = toggle_filter_active_range(&[], "A", 1000.0, 4000.0);
        let out = move_filter_active_span(&events, "A", 1000.0, 4000.0, 100_000.0, 10_000.0);
        let close = out
            .iter()
            .find(|event| event.is_param(Some("A"), "filter", "active") && event.value == Some(0.0))
            .unwrap();
        assert!(close.elapsed_ms <= 10_000.0 + 1e-6);
    }

    #[test]
    fn drawing_the_jog_lane_clears_the_wheel_gesture_it_covers() {
        let events = vec![
            SessionEvent {
                percent: Some(5.0),
                ..make_event(1100.0, "set_nudge", "A")
            },
            SessionEvent {
                ticks: Some(12.0),
                ..make_event(1200.0, "jog", "A")
            },
            SessionEvent {
                ticks: Some(-4.0),
                ..make_event(1300.0, "jog", "A")
            },
            SessionEvent {
                ticks: Some(9.0),
                ..make_event(4000.0, "jog", "A")
            },
        ];
        let out = splice_lane_events(
            &events,
            &jog_lane_spec(),
            "A",
            1000.0,
            2000.0,
            &[lane_point(1000.0, -3.0), lane_point(1500.0, 7.0)],
        );
        let in_range: Vec<&str> = out
            .iter()
            .filter(|event| event.elapsed_ms >= 1000.0 && event.elapsed_ms < 2000.0)
            .map(|event| event.event_type.as_str())
            .collect();
        assert!(!in_range.contains(&"jog"), "got {in_range:?}");
        // The wheel outside the drawn range is untouched.
        assert!(out
            .iter()
            .any(|event| event.event_type == "jog" && event.elapsed_ms == 4000.0));
    }

    #[test]
    fn the_jog_lane_cannot_author_a_deviation_beyond_its_musical_range() {
        let spec = jog_lane_spec();
        assert_eq!(spec.max, 16.0);
        assert_eq!(spec.min, -16.0);

        let out = splice_lane_events(
            &[],
            &spec,
            "A",
            1000.0,
            2000.0,
            &[lane_point(1000.0, 60.0), lane_point(1500.0, -60.0)],
        );
        let drawn: Vec<f64> = out
            .iter()
            .filter(|event| event.event_type == "set_nudge" && event.elapsed_ms < 2000.0)
            .map(|event| event.percent.expect("a nudge carries a percent"))
            .collect();
        assert_eq!(drawn, vec![16.0, -16.0]);
    }

    #[test]
    fn the_jog_lane_writes_what_was_drawn_rather_than_a_difference() {
        let out = splice_lane_events(
            &[],
            &jog_lane_spec(),
            "A",
            1000.0,
            2000.0,
            &[lane_point(1000.0, -3.0), lane_point(1500.0, 7.0)],
        );
        let drawn: Vec<f64> = out
            .iter()
            .filter(|event| event.event_type == "set_nudge" && event.elapsed_ms < 2000.0)
            .map(|event| event.percent.expect("a nudge carries a percent"))
            .collect();
        assert_eq!(drawn, vec![-3.0, 7.0]);
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
        let samples = vec![
            lane_point(200.0, 0.1),
            lane_point(100.0, 0.9),
            lane_point(200.0, 0.5),
        ];
        let out = normalize_gesture_samples(&samples);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ms, 100.0);
        assert_eq!(out[1].ms, 200.0);
        assert_eq!(out[1].value, 0.5);
    }

    #[test]
    fn decimate_drops_steps_below_epsilon() {
        let points = vec![
            lane_point(0.0, 0.0),
            lane_point(1.0, 0.005),
            lane_point(2.0, 0.5),
        ];
        let out = decimate_steps(&points, 0.01);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].value, 0.0);
        assert_eq!(out[1].value, 0.5);
    }

    #[test]
    fn rate_lane_reads_snapshot_and_uses_supplied_range() {
        let spec = rate_lane_spec(Some(0.9), Some(1.1));
        assert_eq!(spec.min, 0.9);
        assert_eq!(spec.max, 1.1);
        let events = vec![SessionEvent {
            playback_rate: Some(0.97),
            ..make_event(0.0, "deck_snapshot", "A")
        }];
        assert!((original_value_at(&events, &spec, "A", 1000.0) - 0.97).abs() < 1e-6);
    }
}

#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::param::CLASSIC_3BAND;

    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn wheel_event(seed: &mut u64, ms: f64, deck: &str) -> SessionEvent {
        SessionEvent {
            ticks: Some((rng(seed) % 41) as f64 - 20.0),
            ..SessionEvent::at(ms, "jog", deck)
        }
    }

    #[test]
    fn a_jog_splice_clears_the_wheel_inside_the_range_and_nothing_outside_it() {
        let mut seed = 0x0DDB_A11B_ADC0_FFEE;
        let spec = jog_lane_spec();

        for _ in 0..2000 {
            let count = (rng(&mut seed) % 8) as usize;
            let events: Vec<SessionEvent> = (0..count)
                .map(|_| {
                    let ms = (rng(&mut seed) % 10_000) as f64;
                    match rng(&mut seed) % 3 {
                        0 => wheel_event(&mut seed, ms, "A"),
                        1 => SessionEvent {
                            percent: Some((rng(&mut seed) % 41) as f64 - 20.0),
                            ..SessionEvent::at(ms, "set_nudge", "A")
                        },
                        _ => wheel_event(&mut seed, ms, "B"),
                    }
                })
                .collect();
            let events = crate::sim::sorted_by_sim_order(events);

            let first = (rng(&mut seed) % 10_000) as f64;
            let second = (rng(&mut seed) % 10_000) as f64;
            let (start, end) = if first <= second {
                (first, second)
            } else {
                (second, first)
            };
            if end - start < MIN_GESTURE_MS {
                continue;
            }

            let points = vec![
                LanePoint {
                    ms: start,
                    value: 3.0,
                },
                LanePoint {
                    ms: end,
                    value: -2.0,
                },
            ];
            let after = splice_lane_events(&events, &spec, "A", start, end, &points);

            for event in &after {
                let inside = event.elapsed_ms >= start && event.elapsed_ms <= end;
                assert!(
                    !(inside && event.event_type == "jog" && event.deck.as_deref() == Some("A")),
                    "a wheel event survived at {} in [{start},{end}]",
                    event.elapsed_ms
                );
            }

            let survives = |original: &SessionEvent| {
                after.iter().any(|event| {
                    event.event_type == original.event_type
                        && event.elapsed_ms == original.elapsed_ms
                        && event.deck == original.deck
                        && event.ticks == original.ticks
                        && event.percent == original.percent
                })
            };

            for original in events
                .iter()
                .filter(|event| event.elapsed_ms < start || event.elapsed_ms > end)
            {
                assert!(
                    survives(original),
                    "an event outside [{start},{end}] was dropped"
                );
            }

            for original in events
                .iter()
                .filter(|event| event.deck.as_deref() == Some("B"))
            {
                assert!(survives(original), "another deck's wheel was cleared");
            }
        }
    }

    #[test]
    fn splice_preserves_lane_value_after_the_edited_range() {
        let mut seed = 0xDEADBEEF12345678u64;
        let deck = "A";
        let mut violations = 0u32;
        let mut examples: Vec<String> = Vec::new();

        for round in 0..4000 {
            let lane = EditableLane::ALL[round % EditableLane::ALL.len()];
            let spec = lane_spec_for(lane, &CLASSIC_3BAND, None, None);
            let pick = |seed: &mut u64| {
                spec.min + (rng(seed) % 101) as f64 / 100.0 * (spec.max - spec.min)
            };
            let n = (rng(&mut seed) % 6) as usize;
            let mut events: Vec<SessionEvent> = (0..n)
                .map(|_| {
                    let ms = (rng(&mut seed) % 10_000) as f64;
                    spec.make_event(ms, pick(&mut seed), deck)
                })
                .collect();
            events = crate::sim::sorted_by_sim_order(events);

            let first = (rng(&mut seed) % 10_000) as f64;
            let second = (rng(&mut seed) % 10_000) as f64;
            let (range_start, range_end) = if first <= second {
                (first, second)
            } else {
                (second, first)
            };

            // Deliberately unordered: splice must not assume sorted input.
            let count = 1 + (rng(&mut seed) % 5) as usize;
            let points: Vec<LanePoint> = (0..count)
                .map(|_| LanePoint {
                    ms: range_start
                        + (rng(&mut seed) % ((range_end - range_start).max(1.0) as u64 + 1)) as f64,
                    value: pick(&mut seed),
                })
                .collect();

            let after = splice_lane_events(&events, &spec, deck, range_start, range_end, &points);

            for pair in after.windows(2) {
                assert!(
                    pair[0].elapsed_ms <= pair[1].elapsed_ms,
                    "spliced events must stay ordered"
                );
            }

            for probe in [
                range_end + 0.001,
                range_end + 1.0,
                range_end + 500.0,
                10_500.0,
            ] {
                let before_value = original_value_at(&events, &spec, deck, probe);
                let after_value = original_value_at(&after, &spec, deck, probe);
                if (before_value - after_value).abs() > 1e-9 {
                    violations += 1;
                    if examples.len() < 3 {
                        examples.push(format!(
                            "{lane:?} range=[{range_start},{range_end}] probe={probe} before={before_value} after={after_value}"
                        ));
                    }
                    break;
                }
            }
        }

        for example in &examples {
            println!("   {example}");
        }
        assert_eq!(
            violations, 0,
            "splice changed values after the edited range"
        );
    }
}
