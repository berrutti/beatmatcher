pub mod clip_edit;
pub mod cue;
pub mod event;
pub mod lane_edit;
pub mod param;
pub mod sim;
pub mod timeline;

pub use clip_edit::{
    block_bounds, blocks_for_deck, delete_block_range, delete_transport_block,
    delete_transport_ranges, move_transport_block, split_transport_block, trim_transport_block,
    DeleteRange, Edge, MoveResult, TransportBlock, TrimResult, MIN_BLOCK_MS,
};
pub use cue::{build_cue_points, CuePoint};
pub use event::{port_events, SessionCommand, SessionEvent, SessionFile, BMS_VERSION};
pub use lane_edit::{
    decimate_steps, delete_filter_active_span, filter_active_at, lane_move_span, lane_spec_for,
    move_filter_active_span, normalize_gesture_samples, original_value_at, rate_lane_spec,
    relocate_event_paths, reset_lane_from, resize_filter_active_span, set_rate_at, set_rate_span,
    splice_lane_events, toggle_filter_active_range, EditableLane, LaneDisplay, LaneSpec,
    ResetExtent, EQ_MAX_DB, EQ_MIN_DB, FILTER_DEAD_ZONE, MIN_GESTURE_MS,
};
pub use param::{
    is_fader_gain, jog_settled_fraction, manifest_by_id, resolve_manifest, xfader_gains,
    FaderCurve, JogRotationSpeed, MixerHeader, MixerManifest, ParamDescriptor, ParamScope,
    ParamUnit, SlotDescriptor, Taper, XfaderAssign, CLASSIC_3BAND, CLASSIC_3BAND_V2, FADER_GAIN,
    ISOLATOR_3BAND, ISOLATOR_3BAND_V2, JOG_FILTER_TAU_SEC, JOG_PAUSED_MULTIPLIER,
    JOG_SCRUB_SEC_PER_TICK_AT_33, JOG_SHIFT_MULTIPLIER, MANIFESTS, REQUIRED_STRIP_ROLES,
};
pub use sim::{
    build_snapshots, current_beat, event_sim_order, sim_apply_event, sim_pos,
    sim_state_from_snapshot, DeckSim, DeckSnap, SampleCache, SessionSnapshot, SimState, StripSim,
    DEFAULT_MASTER_GAIN, JOG_FACTOR_MIN,
};
pub use timeline::{
    build_clips, build_lanes, build_timeline, Clip, ClipsBuild, DeckLanes, FilterActiveSpan,
    LanePoint, LanesBuild, LoadedSpan, LoopRegion, MasterLanes, TimelineBuild, WaveSeg,
};

// WASM boundary for the frontend. Pure compute only: events in as JSON, the
// derived timeline out as JSON. No side effects (file/audio/IPC stay in Rust
// proper). The frontend parses the returned JSON. The serde camelCase derives
// on the result structs make it match the existing TS shapes 1:1.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    fn parse_events(events_json: &str) -> Result<Vec<crate::SessionEvent>, JsError> {
        serde_json::from_str(events_json).map_err(|error| JsError::new(&error.to_string()))
    }

    /// One pass, so the editor crosses the WASM boundary and serializes the event list
    /// once per change rather than once per lane.
    #[wasm_bindgen(js_name = buildTimeline)]
    pub fn build_timeline(
        events_json: &str,
        duration_ms: f64,
        pitch_options: &[f64],
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let result = crate::build_timeline(&events, duration_ms, pitch_options);
        serde_json::to_string(&result).map_err(|error| JsError::new(&error.to_string()))
    }

    /// Mirrors the engine math so no consumer reimplements it.
    #[wasm_bindgen(js_name = currentBeat)]
    pub fn current_beat(position_sec: f64, beat_offset_sec: f64, bpm: f64) -> f64 {
        crate::current_beat(position_sec, beat_offset_sec, bpm)
    }

    fn parse_clips(clips_json: &str) -> Result<Vec<crate::Clip>, JsError> {
        serde_json::from_str(clips_json).map_err(|error| JsError::new(&error.to_string()))
    }

    fn parse_block(block_json: &str) -> Result<crate::TransportBlock, JsError> {
        serde_json::from_str(block_json).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = blocksForDeck)]
    pub fn blocks_for_deck(clips_json: &str, deck: &str) -> Result<String, JsError> {
        let clips = parse_clips(clips_json)?;
        let blocks = crate::blocks_for_deck(&clips, deck);
        serde_json::to_string(&blocks).map_err(|error| JsError::new(&error.to_string()))
    }

    /// Drag-clamp range for a block: `{ minStartMs, maxEndMs, startTrimMinMs,
    /// minBlockMs }`. A null `maxEndMs` means open-ended. `startTrimMinMs` uses the
    /// trim commit's own formula so preview and commit clamp identically.
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
            let earliest_by_audio = if block.playback_rate > 0.0 {
                block.start_ms - (block.track_start_sec / block.playback_rate) * 1000.0
            } else {
                min_start
            };
            serde_json::json!({
                "minStartMs": min_start,
                "maxEndMs": if max_end.is_finite() { Some(max_end) } else { None },
                "startTrimMinMs": min_start.max(earliest_by_audio),
                "minBlockMs": crate::MIN_BLOCK_MS,
            })
        });
        serde_json::to_string(&out).map_err(|error| JsError::new(&error.to_string()))
    }

    // A build that dropped a mixer still has to draw a session naming it, so the editor
    // falls back. Rendering audio does not: `resolve_manifest` is strict on that path.
    fn resolve_mixer(id: &str) -> &'static crate::MixerManifest {
        crate::manifest_by_id(id).unwrap_or(&crate::CLASSIC_3BAND)
    }

    /// Rate carries its default range: a caller with a clip-specific one overrides
    /// min/max rather than reading it from here.
    #[wasm_bindgen(js_name = laneSpecs)]
    pub fn lane_specs(mixer_id: &str) -> String {
        let mixer = resolve_mixer(mixer_id);
        let map: serde_json::Map<String, serde_json::Value> = crate::EditableLane::ALL
            .into_iter()
            .map(|lane| {
                let spec = crate::lane_spec_for(lane, mixer, None, None);
                let display = lane.display(mixer);
                (
                    lane.key().to_string(),
                    serde_json::json!({
                        "key": lane.key(),
                        "min": spec.min,
                        "max": spec.max,
                        "defaultValue": spec.default_value,
                        "epsilon": spec.epsilon,
                        "unit": display.unit,
                    }),
                )
            })
            .collect();
        serde_json::Value::Object(map).to_string()
    }

    /// A mixer's deck-scope params, keyed `"slot/param"`, so knobs take their range from
    /// the running manifest. An unknown id falls back to the classic mixer.
    #[wasm_bindgen(js_name = mixerParams)]
    pub fn mixer_params(id: &str) -> String {
        let manifest = resolve_mixer(id);
        let map: serde_json::Map<String, serde_json::Value> = manifest
            .strip
            .iter()
            .flat_map(|slot| slot.params.iter().map(move |param| (slot.slot, param)))
            .map(|(slot, param)| {
                (
                    format!("{slot}/{}", param.id),
                    serde_json::json!({
                        "slot": slot,
                        "param": param.id,
                        "min": param.min,
                        "max": param.max,
                        "defaultValue": param.default,
                        "step": param.step,
                    }),
                )
            })
            .collect();
        serde_json::Value::Object(map).to_string()
    }

    #[wasm_bindgen(js_name = bmsVersion)]
    pub fn bms_version() -> u32 {
        crate::BMS_VERSION
    }

    /// The gain a curve puts on a fader position, so a caller can show the taper
    /// the engine actually applies without reimplementing it.
    #[wasm_bindgen(js_name = faderCurveGain)]
    pub fn fader_curve_gain(curve: &str, position: f64) -> f64 {
        crate::FaderCurve::from_str_or_linear(curve).gain(position)
    }

    #[wasm_bindgen(js_name = editConstants)]
    pub fn edit_constants() -> String {
        serde_json::json!({
            "eqMinDb": crate::EQ_MIN_DB,
            "eqMaxDb": crate::EQ_MAX_DB,
            "filterDeadZone": crate::FILTER_DEAD_ZONE,
            "defaultMasterGain": crate::DEFAULT_MASTER_GAIN,
            "minBlockMs": crate::MIN_BLOCK_MS,
            "minGestureMs": crate::MIN_GESTURE_MS,
        })
        .to_string()
    }

    /// Returns `{ events, appliedDeltaMs }`: the delta is clamped to the block's
    /// neighborhood, so the caller cannot assume it got the one it asked for.
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
        serde_json::to_string(&result).map_err(|error| JsError::new(&error.to_string()))
    }

    /// Returns `{ events, appliedMs }`: the edge is clamped, so the caller cannot
    /// assume it landed on `new_ms`.
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
        serde_json::to_string(&result).map_err(|error| JsError::new(&error.to_string()))
    }

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

    /// Gapless: the right part resumes exactly the audio it already played.
    /// A no-op if `split_ms` is within `minBlockMs` of either edge.
    #[wasm_bindgen(js_name = splitTransportBlock)]
    pub fn split_transport_block(
        events_json: &str,
        clips_json: &str,
        block_json: &str,
        split_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let clips = parse_clips(clips_json)?;
        let block = parse_block(block_json)?;
        events_to_json(crate::split_transport_block(
            &events, &clips, &block, split_ms,
        ))
    }

    /// One edit, because a range covering a whole block deletes it, an edge range
    /// trims and an interior range splits: applied singly they would fight.
    #[wasm_bindgen(js_name = deleteTransportRanges)]
    pub fn delete_transport_ranges(
        events_json: &str,
        clips_json: &str,
        ranges_json: &str,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let clips = parse_clips(clips_json)?;
        let ranges: Vec<crate::DeleteRange> =
            serde_json::from_str(ranges_json).map_err(|error| JsError::new(&error.to_string()))?;
        events_to_json(crate::delete_transport_ranges(&events, &clips, &ranges))
    }

    fn parse_points(points_json: &str) -> Result<Vec<crate::LanePoint>, JsError> {
        serde_json::from_str(points_json).map_err(|error| JsError::new(&error.to_string()))
    }

    fn parse_lane(lane_key: &str) -> Result<crate::EditableLane, JsError> {
        crate::EditableLane::from_key(lane_key)
            .ok_or_else(|| JsError::new(&format!("invalid lane: {lane_key}")))
    }

    fn lane_spec(
        lane_key: &str,
        mixer_id: &str,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<crate::LaneSpec, JsError> {
        Ok(crate::lane_spec_for(
            parse_lane(lane_key)?,
            resolve_mixer(mixer_id),
            Some(rate_min),
            Some(rate_max),
        ))
    }

    fn events_to_json(events: Vec<crate::SessionEvent>) -> Result<String, JsError> {
        serde_json::to_string(&events).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = normalizeGestureSamples)]
    pub fn normalize_gesture_samples(points_json: &str) -> Result<String, JsError> {
        let points = parse_points(points_json)?;
        let out = crate::normalize_gesture_samples(&points);
        serde_json::to_string(&out).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decimateSteps)]
    pub fn decimate_steps(points_json: &str, epsilon: f64) -> Result<String, JsError> {
        let points = parse_points(points_json)?;
        let out = crate::decimate_steps(&points, epsilon);
        serde_json::to_string(&out).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = originalValueAt)]
    pub fn original_value_at(
        events_json: &str,
        lane_key: &str,
        mixer_id: &str,
        deck: &str,
        ms: f64,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<f64, JsError> {
        let events = parse_events(events_json)?;
        let spec = lane_spec(lane_key, mixer_id, rate_min, rate_max)?;
        Ok(crate::original_value_at(&events, &spec, deck, ms))
    }

    /// The span `ResetExtent::ThisMove` would clear, as `{startMs, endMs}`, both
    /// inclusive. Null when the lane is already at rest there.
    #[wasm_bindgen(js_name = laneMoveSpan)]
    pub fn lane_move_span(
        events_json: &str,
        lane_key: &str,
        mixer_id: &str,
        deck: &str,
        ms: f64,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let spec = lane_spec(lane_key, mixer_id, rate_min, rate_max)?;
        let span = crate::lane_move_span(&events, &spec, deck, ms)
            .map(|(start, end)| serde_json::json!({ "startMs": start, "endMs": end }));
        serde_json::to_string(&span).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = resetLaneFrom)]
    pub fn reset_lane_from(
        events_json: &str,
        lane_key: &str,
        mixer_id: &str,
        deck: &str,
        ms: f64,
        extent: &str,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let spec = lane_spec(lane_key, mixer_id, rate_min, rate_max)?;
        let extent = match extent {
            "toEnd" => crate::ResetExtent::ToEnd,
            "untilHere" => crate::ResetExtent::UntilHere,
            "thisMove" => crate::ResetExtent::ThisMove,
            other => return Err(JsError::new(&format!("unknown reset extent: {other}"))),
        };
        events_to_json(crate::reset_lane_from(&events, &spec, deck, ms, extent))
    }

    /// Restores at `range_end_ms`, so audio after the gesture is unchanged.
    #[wasm_bindgen(js_name = spliceLaneEvents)]
    #[allow(clippy::too_many_arguments)]
    pub fn splice_lane_events(
        events_json: &str,
        lane_key: &str,
        mixer_id: &str,
        deck: &str,
        range_start_ms: f64,
        range_end_ms: f64,
        points_json: &str,
        rate_min: f64,
        rate_max: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let spec = lane_spec(lane_key, mixer_id, rate_min, rate_max)?;
        let points = parse_points(points_json)?;
        events_to_json(crate::splice_lane_events(
            &events,
            &spec,
            deck,
            range_start_ms,
            range_end_ms,
            &points,
        ))
    }

    /// The new rate holds until the next existing change.
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

    /// Restores the pre-edit rate after the span. Backs "Set BPM (whole clip)".
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

    /// Restores the original state at `range_end_ms`.
    #[wasm_bindgen(js_name = toggleFilterActiveRange)]
    pub fn toggle_filter_active_range(
        events_json: &str,
        deck: &str,
        range_start_ms: f64,
        range_end_ms: f64,
    ) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        events_to_json(crate::toggle_filter_active_range(
            &events,
            deck,
            range_start_ms,
            range_end_ms,
        ))
    }

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

    /// Rewrite event track paths per `mapping` (JSON object old->new path).
    /// JSON `null` = no event carries a mapped path.
    #[wasm_bindgen(js_name = relocateEventPaths)]
    pub fn relocate_event_paths(events_json: &str, mapping_json: &str) -> Result<String, JsError> {
        let events = parse_events(events_json)?;
        let mapping: std::collections::HashMap<String, String> =
            serde_json::from_str(mapping_json).map_err(|error| JsError::new(&error.to_string()))?;
        match crate::relocate_event_paths(&events, &mapping) {
            Some(edited) => events_to_json(edited),
            None => Ok("null".to_string()),
        }
    }
}
