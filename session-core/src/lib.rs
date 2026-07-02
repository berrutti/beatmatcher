// Shared session-replay core: the single source of truth for the event model
// and the deterministic simulation that derives deck/strip state over time.
// Consumed natively by the audio engine (src-tauri) and, once wired, compiled
// to WASM for the frontend, so the engine and the editor can never disagree.

pub mod clip_edit;
pub mod cue;
pub mod event;
pub mod lane_edit;
pub mod sim;
pub mod timeline;

pub use cue::{build_cue_points, CuePoint};
pub use clip_edit::{
    block_bounds, blocks_for_deck, delete_transport_block, move_transport_block,
    trim_transport_block, Edge, MoveResult, TransportBlock, TrimResult, MIN_BLOCK_MS,
};
pub use event::{SessionCommand, SessionEvent, SessionFile};
pub use lane_edit::{
    decimate_steps, delete_filter_active_span, delete_nudge_range, filter_active_at, lane_spec_for,
    move_filter_active_span, normalize_gesture_samples, nudge_value_at, original_value_at,
    paint_nudge_range, relocate_event_paths, resize_filter_active_span, set_rate_at, set_rate_span,
    splice_lane_events, toggle_filter_active_range,
    EditableLane, LaneSpec, MIN_GESTURE_MS,
};
pub use sim::{
    build_snapshots, current_beat, event_sim_order, sim_apply_event, sim_pos,
    sim_state_from_snapshot, DeckSim, DeckSnap, SampleCache, SessionSnapshot, SimState, StripSim,
    StripSnap, DEFAULT_MASTER_GAIN,
};
pub use timeline::{
    build_clips, build_lanes, build_timeline, Clip, ClipsBuild, DeckLanes, FilterActiveSpan,
    LanePoint, LanesBuild, LoadedSpan, LoopRegion, MasterLanes, NudgeSpan, TimelineBuild, WaveSeg,
};

// WASM boundary for the frontend. Pure compute only: events in as JSON, the
// derived timeline out as JSON. No side effects (file/audio/IPC stay in Rust
// proper). The frontend parses the returned JSON; the serde camelCase derives
// on the result structs make it match the existing TS shapes 1:1.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    fn parse_events(events_json: &str) -> Result<Vec<crate::SessionEvent>, JsError> {
        serde_json::from_str(events_json).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Derive clips, loaded spans, and automation lanes (gain/eq/filter/rate,
    /// filter-active spans, nudge spans) in one pass so the editor crosses the
    /// boundary (and serializes the event list) once per change. `trackName` is
    /// not included on clips/spans; the caller fills it from the collection.
    /// Returns `{ clips, loadedSpans, deckLanes, masterLanes, deckNudges }`.
    #[wasm_bindgen(js_name = buildTimeline)]
    pub fn build_timeline(
        events_json: &str,
        duration_ms: f64,
        pitch_options: &[f64],
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let result = crate::build_timeline(&events, duration_ms, pitch_options);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Continuous beat count at a playback position given the track's beat grid.
    /// Mirrors the engine math so the phase ring (and any consumer) never
    /// reimplements it. Primitives in/out, no JSON.
    #[wasm_bindgen(js_name = currentBeat)]
    pub fn current_beat(position_sec: f64, beat_offset_sec: f64, bpm: f64) -> f64 {
        crate::current_beat(position_sec, beat_offset_sec, bpm)
    }

    fn parse_clips(clips_json: &str) -> Result<Vec<crate::Clip>, JsError> {
        serde_json::from_str(clips_json).map_err(|e| JsError::new(&e.to_string()))
    }

    fn parse_block(block_json: &str) -> Result<crate::TransportBlock, JsError> {
        serde_json::from_str(block_json).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Group a deck's clips into draggable transport blocks. Returns a JSON
    /// array of blocks (camelCase, `loop` field) sorted by start.
    #[wasm_bindgen(js_name = blocksForDeck)]
    pub fn blocks_for_deck(clips_json: &str, deck: &str) -> Result<String, JsError> {
        let clips = parse_clips(clips_json)?;
        let blocks = crate::blocks_for_deck(&clips, deck);
        serde_json::to_string(&blocks).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Drag-clamp range for a block. Returns `{ minStartMs, maxEndMs }`, with
    /// `maxEndMs` = null meaning open-ended (Rust Infinity has no JSON form).
    #[wasm_bindgen(js_name = blockBounds)]
    pub fn block_bounds(
        events_json: &str,
        clips_json: &str,
        block_json: &str,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let clips = parse_clips(clips_json)?;
        let block = parse_block(block_json)?;
        let out = crate::block_bounds(&events, &clips, &block).map(|(min_start, max_end)| {
            serde_json::json!({
                "minStartMs": min_start,
                "maxEndMs": if max_end.is_finite() { Some(max_end) } else { None },
            })
        });
        serde_json::to_string(&out).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Move a block by `delta_ms` (clamped to its neighborhood). Returns
    /// `{ events, appliedDeltaMs }` JSON.
    #[wasm_bindgen(js_name = moveTransportBlock)]
    pub fn move_transport_block(
        events_json: &str,
        clips_json: &str,
        block_json: &str,
        delta_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let clips = parse_clips(clips_json)?;
        let block = parse_block(block_json)?;
        let result = crate::move_transport_block(&events, &clips, &block, delta_ms);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Trim a block's `"start"` or `"end"` edge to `new_ms`. Returns
    /// `{ events, appliedMs }` JSON.
    #[wasm_bindgen(js_name = trimTransportBlock)]
    pub fn trim_transport_block(
        events_json: &str,
        clips_json: &str,
        block_json: &str,
        edge: &str,
        new_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let clips = parse_clips(clips_json)?;
        let block = parse_block(block_json)?;
        let edge = match edge {
            "start" => crate::Edge::Start,
            "end" => crate::Edge::End,
            other => return Err(JsError::new(&format!("invalid edge: {other}"))),
        };
        let result = crate::trim_transport_block(&events, &clips, &block, edge, new_ms);
        serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Delete a transport block (drop its play/stop). Returns the events JSON.
    #[wasm_bindgen(js_name = deleteTransportBlock)]
    pub fn delete_transport_block(
        events_json: &str,
        clips_json: &str,
        block_json: &str,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let clips = parse_clips(clips_json)?;
        let block = parse_block(block_json)?;
        events_to_json(crate::delete_transport_block(&events, &clips, &block))
    }

    fn parse_points(points_json: &str) -> Result<Vec<crate::LanePoint>, JsError> {
        serde_json::from_str(points_json).map_err(|e| JsError::new(&e.to_string()))
    }

    fn parse_lane(lane_key: &str) -> Result<crate::EditableLane, JsError> {
        crate::EditableLane::from_key(lane_key)
            .ok_or_else(|| JsError::new(&format!("invalid lane: {lane_key}")))
    }

    fn lane_spec(lane_key: &str, rate_min: f64, rate_max: f64) -> Result<crate::LaneSpec, JsError> {
        Ok(crate::lane_spec_for(
            parse_lane(lane_key)?,
            Some(rate_min),
            Some(rate_max),
        ))
    }

    fn events_to_json(events: Vec<crate::SessionEvent>) -> Result<String, JsError> {
        serde_json::to_string(&events).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Dedupe gesture samples by ms (last wins) and sort. JSON `LanePoint[]`.
    #[wasm_bindgen(js_name = normalizeGestureSamples)]
    pub fn normalize_gesture_samples(points_json: &str) -> Result<String, JsError> {
        let points = parse_points(points_json)?;
        let out = crate::normalize_gesture_samples(&points);
        serde_json::to_string(&out).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Drop points whose value-step from the last kept point is below `epsilon`.
    #[wasm_bindgen(js_name = decimateSteps)]
    pub fn decimate_steps(points_json: &str, epsilon: f64) -> Result<String, JsError> {
        let points = parse_points(points_json)?;
        let out = crate::decimate_steps(&points, epsilon);
        serde_json::to_string(&out).map_err(|e| JsError::new(&e.to_string()))
    }

    /// The lane's effective value at `ms` (the last point at/before it, else default).
    #[wasm_bindgen(js_name = originalValueAt)]
    pub fn original_value_at(
        events_json: &str,
        lane_key: &str,
        deck: &str,
        ms: f64,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<f64, JsError> {
        let events = parse_events(events_json)?;
        let spec = lane_spec(lane_key, rate_min, rate_max)?;
        Ok(crate::original_value_at(&events, &spec, deck, ms))
    }

    /// Replace lane events in [t0, t1] with the drawn points; restore at t1.
    #[wasm_bindgen(js_name = spliceLaneEvents)]
    #[allow(clippy::too_many_arguments)]
    pub fn splice_lane_events(
        events_json: &str,
        lane_key: &str,
        deck: &str,
        t0: f64,
        t1: f64,
        points_json: &str,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let spec = lane_spec(lane_key, rate_min, rate_max)?;
        let points = parse_points(points_json)?;
        events_to_json(crate::splice_lane_events(
            &events, &spec, deck, t0, t1, &points,
        ))
    }

    /// Insert (or replace) a single set_playback_rate point for `deck` at `ms`.
    /// The new rate holds until the next existing change. Returns events JSON.
    #[wasm_bindgen(js_name = setRateAt)]
    pub fn set_rate_at(
        events_json: &str,
        deck: &str,
        ms: f64,
        rate: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::set_rate_at(&events, deck, ms, rate))
    }

    /// Set one uniform rate over [start_ms, end_ms], restoring the pre-edit rate
    /// after. Backs "Set BPM (whole clip)". Returns events JSON.
    #[wasm_bindgen(js_name = setRateSpan)]
    pub fn set_rate_span(
        events_json: &str,
        deck: &str,
        start_ms: f64,
        end_ms: f64,
        rate: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::set_rate_span(&events, deck, start_ms, end_ms, rate))
    }

    /// Filter on/off state for `deck` at `ms`.
    #[wasm_bindgen(js_name = filterActiveAt)]
    pub fn filter_active_at(
        events_json: &str,
        deck: &str,
        ms: f64,
        inclusive: bool,
    ) -> Result<bool, JsError> {
        let events = parse_events(events_json)?;
        Ok(crate::filter_active_at(&events, deck, ms, inclusive))
    }

    /// Toggle the filter on/off over [t0, t1], restoring the original state at t1.
    #[wasm_bindgen(js_name = toggleFilterActiveRange)]
    pub fn toggle_filter_active_range(
        events_json: &str,
        deck: &str,
        t0: f64,
        t1: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::toggle_filter_active_range(&events, deck, t0, t1))
    }

    /// Delete the filter-active span [start_ms, end_ms] (its on/off event pair).
    #[wasm_bindgen(js_name = deleteFilterActiveSpan)]
    pub fn delete_filter_active_span(
        events_json: &str,
        deck: &str,
        start_ms: f64,
        end_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::delete_filter_active_span(
            &events, deck, start_ms, end_ms,
        ))
    }

    /// Stretch the `start` or `end` edge of the filter-active span to `new_ms`.
    #[wasm_bindgen(js_name = resizeFilterActiveSpan)]
    #[allow(clippy::too_many_arguments)]
    pub fn resize_filter_active_span(
        events_json: &str,
        deck: &str,
        start_ms: f64,
        end_ms: f64,
        edge: &str,
        new_ms: f64,
        duration_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::resize_filter_active_span(
            &events,
            deck,
            start_ms,
            end_ms,
            edge,
            new_ms,
            duration_ms,
        ))
    }

    /// Slide the whole filter-active span [`start_ms`, `end_ms`] by `delta_ms`.
    #[wasm_bindgen(js_name = moveFilterActiveSpan)]
    #[allow(clippy::too_many_arguments)]
    pub fn move_filter_active_span(
        events_json: &str,
        deck: &str,
        start_ms: f64,
        end_ms: f64,
        delta_ms: f64,
        duration_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::move_filter_active_span(
            &events,
            deck,
            start_ms,
            end_ms,
            delta_ms,
            duration_ms,
        ))
    }

    /// The nudge percent active for `deck` at `ms` (0 when none).
    #[wasm_bindgen(js_name = nudgeValueAt)]
    pub fn nudge_value_at(
        events_json: &str,
        deck: &str,
        ms: f64,
        inclusive: bool,
    ) -> Result<f64, JsError> {
        let events = parse_events(events_json)?;
        Ok(crate::nudge_value_at(&events, deck, ms, inclusive))
    }

    /// Paint a nudge `percent` over [t0, t1], restoring the recorded value at t1.
    #[wasm_bindgen(js_name = paintNudgeRange)]
    pub fn paint_nudge_range(
        events_json: &str,
        deck: &str,
        t0: f64,
        t1: f64,
        percent: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::paint_nudge_range(&events, deck, t0, t1, percent))
    }

    /// Remove the nudge span for `deck` in [t0, t1] (keeps an adjacent opener at t1).
    #[wasm_bindgen(js_name = deleteNudgeRange)]
    pub fn delete_nudge_range(
        events_json: &str,
        deck: &str,
        t0: f64,
        t1: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::delete_nudge_range(&events, deck, t0, t1))
    }

    /// Rewrite event track paths per `mapping` (JSON object old->new path).
    #[wasm_bindgen(js_name = relocateEventPaths)]
    pub fn relocate_event_paths(events_json: &str, mapping_json: &str) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let mapping: std::collections::HashMap<String, String> =
            serde_json::from_str(mapping_json).map_err(|e| JsError::new(&e.to_string()))?;
        events_to_json(crate::relocate_event_paths(&events, &mapping))
    }
}
