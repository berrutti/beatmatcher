// Golden end-to-end regression: a hand-computed two-deck session through the
// timeline derivation, scripted edits, and scrub-style playback reconstruction.

use session_core::{
    blocks_for_deck, build_snapshots, build_timeline, delete_transport_ranges, event_sim_order,
    lane_spec_for, move_transport_block, sim_apply_event, sim_pos, sim_state_from_snapshot,
    splice_lane_events, Clip, DeleteRange, EditableLane, SampleCache, SessionEvent, SimState,
};
use std::collections::HashMap;
use std::sync::Arc;

const SAMPLE_RATE: u32 = 44_100;
const SAMPLE_RATE_F64: f64 = 44_100.0;
const PITCH_OPTS: [f64; 6] = [6.0, 8.0, 10.0, 16.0, 50.0, 100.0];

fn make_event(ms: f64, event_type: &str, deck: &str) -> SessionEvent {
    SessionEvent::at(ms, event_type, deck)
}

// Deck A: straight play with a mid-clip rate change. Deck B: play into a
// two-iteration loop, exit, tail. All positions are integer-second friendly.
fn fixture_events() -> Vec<SessionEvent> {
    vec![
        SessionEvent {
            path: Some("/t/a.mp3".to_string()),
            ..make_event(0.0, "load_track", "A")
        },
        SessionEvent {
            path: Some("/t/b.mp3".to_string()),
            ..make_event(0.0, "load_track", "B")
        },
        SessionEvent {
            bpm: Some(120.0),
            beat_offset_sec: Some(0.0),
            ..make_event(100.0, "set_beat_grid", "A")
        },
        make_event(1000.0, "play", "B"),
        make_event(2000.0, "play", "A"),
        SessionEvent {
            start_sec: Some(1.0),
            end_sec: Some(2.0),
            ..make_event(3000.0, "loop_out", "B")
        },
        SessionEvent {
            slot: Some("fader".to_string()),
            param: Some("gain".to_string()),
            value: Some(0.5),
            ..make_event(4000.0, "set_param", "A")
        },
        make_event(5000.0, "exit_loop", "B"),
        SessionEvent {
            rate: Some(1.5),
            ..make_event(6000.0, "set_playback_rate", "A")
        },
        make_event(7000.0, "stop", "B"),
        make_event(10_000.0, "stop", "A"),
    ]
}

fn track_cache() -> SampleCache {
    let mut cache: SampleCache = HashMap::new();
    for path in ["/t/a.mp3", "/t/b.mp3"] {
        cache.insert(
            path.to_string(),
            (Arc::new(vec![0.0f32; 60 * SAMPLE_RATE as usize * 2]), 2),
        );
    }
    cache
}

fn clips_for(events: &[SessionEvent]) -> Vec<Clip> {
    build_timeline(events, 12_000.0, &PITCH_OPTS).clips
}

struct Reconstructed {
    is_playing: bool,
    path: Option<String>,
    position_frame: f64,
    strip_gain: f32,
}

// The scrub placement path, exactly as start_session_playback does it.
fn reconstruct(events: &[SessionEvent], from_ms: f64, deck: &str) -> Option<Reconstructed> {
    let cache = track_cache();
    let snaps = build_snapshots(events, SAMPLE_RATE, &cache);
    let idx = snaps.partition_point(|snap| snap.elapsed_ms <= from_ms);
    let (mut sim, snapshot_ms) = match idx.checked_sub(1) {
        Some(snap_idx) => (
            sim_state_from_snapshot(&snaps[snap_idx]),
            snaps[snap_idx].elapsed_ms,
        ),
        None => (SimState::new(), 0.0),
    };
    let mut sorted: Vec<&SessionEvent> = events.iter().collect();
    sorted.sort_by(|first, second| event_sim_order(first, second));
    for event in sorted.iter().filter(|event| {
        event.elapsed_ms > snapshot_ms
            && event.elapsed_ms <= from_ms
            && event.event_type != "deck_snapshot"
    }) {
        sim_apply_event(event, &mut sim, &cache, SAMPLE_RATE);
    }
    let strip_gain = sim.strips.get(deck).map(|strip| strip.gain).unwrap_or(1.0);
    sim.decks.get(deck).map(|deck_sim| Reconstructed {
        is_playing: deck_sim.is_playing,
        path: deck_sim.path.clone(),
        position_frame: sim_pos(deck_sim, from_ms, SAMPLE_RATE_F64),
        strip_gain,
    })
}

fn assert_position_sec(events: &[SessionEvent], from_ms: f64, deck: &str, expected_sec: f64) {
    let state = reconstruct(events, from_ms, deck).expect("deck missing");
    assert!(state.is_playing, "{deck} must play at {from_ms}ms");
    let expected = expected_sec * SAMPLE_RATE_F64;
    assert!(
        (state.position_frame - expected).abs() < 2.0,
        "{deck} at {from_ms}ms: expected {expected_sec}s, got {}s",
        state.position_frame / SAMPLE_RATE_F64
    );
}

fn span(clip: &Clip) -> (f64, f64, f64) {
    (
        clip.session_start_ms,
        clip.session_end_ms,
        clip.track_start_sec,
    )
}

fn assert_span(clip: &Clip, start_ms: f64, end_ms: f64, track_start_sec: f64) {
    let (actual_start, actual_end, actual_track) = span(clip);
    assert!(
        (actual_start - start_ms).abs() < 1.0
            && (actual_end - end_ms).abs() < 1.0
            && (actual_track - track_start_sec).abs() < 1e-6,
        "expected [{start_ms}, {end_ms}] track {track_start_sec}, got {:?}",
        span(clip)
    );
}

#[test]
fn golden_session_derives_edits_and_reconstructs() {
    let events = fixture_events();

    // Initial derivation.
    let clips = clips_for(&events);
    let deck_a: Vec<&Clip> = clips.iter().filter(|clip| clip.deck == "A").collect();
    assert_eq!(deck_a.len(), 1);
    assert_span(deck_a[0], 2000.0, 10_000.0, 0.0);
    let segments = &deck_a[0].wave_segments;
    assert_eq!(segments.len(), 2, "rate change must split the wave segments");
    assert!((segments[0].track_end_sec - 4.0).abs() < 1e-6);
    assert!((segments[1].track_end_sec - 10.0).abs() < 1e-6);
    assert_eq!(deck_a[0].bpm, Some(120.0));

    let deck_b: Vec<&Clip> = clips.iter().filter(|clip| clip.deck == "B").collect();
    assert_eq!(deck_b.len(), 4, "pre-loop + 2 iterations + tail: {deck_b:?}");
    assert_span(deck_b[0], 1000.0, 3000.0, 0.0);
    assert_span(deck_b[1], 3000.0, 4000.0, 1.0);
    assert_span(deck_b[2], 4000.0, 5000.0, 1.0);
    assert_span(deck_b[3], 5000.0, 7000.0, 1.0);

    // Reconstruction of the untouched session: 4s at 1.0x, then 1s at 1.5x.
    assert_position_sec(&events, 7000.0, "A", 4.0 + 1.0 * 1.5);
    assert_position_sec(&events, 4500.0, "B", 1.5);

    // Edit 1: move deck A's block one second right. Automation stays at wall
    // time, so the rate still changes at 6000ms: 3s at 1.0x, then 1.5x.
    let result = move_transport_block(&events, &clips, &blocks_for_deck(&clips, "A")[0], 1000.0);
    assert!((result.applied_delta_ms - 1000.0).abs() < 1e-9);
    let events = result.events;
    let clips = clips_for(&events);
    let moved: Vec<&Clip> = clips.iter().filter(|clip| clip.deck == "A").collect();
    assert_eq!(moved.len(), 1);
    assert_span(moved[0], 3000.0, 11_000.0, 0.0);
    assert_position_sec(&events, 7000.0, "A", 3.0 + 1.0 * 1.5);
    let before_start = reconstruct(&events, 2500.0, "A").expect("deck missing");
    assert!(!before_start.is_playing, "A starts at 3000ms after the move");

    // Edit 2: delete [3500, 4500] out of deck B's loop run; it must re-engage
    // at the exact in-loop position (1.5s) so surviving iterations keep phase.
    let events = delete_transport_ranges(
        &events,
        &clips,
        &[DeleteRange {
            deck: "B".to_string(),
            start_ms: 3500.0,
            end_ms: 4500.0,
        }],
    );
    let clips = clips_for(&events);
    let deck_b: Vec<&Clip> = clips.iter().filter(|clip| clip.deck == "B").collect();
    assert!(!deck_b
        .iter()
        .any(|clip| clip.session_start_ms > 3501.0 && clip.session_start_ms < 4499.0));
    let resumed = deck_b
        .iter()
        .find(|clip| (clip.session_start_ms - 4500.0).abs() < 1.0)
        .expect("resumed loop clip");
    assert!(resumed.loop_region.is_some());
    assert!((resumed.track_start_sec - 1.5).abs() < 1e-6);
    let tail = deck_b
        .iter()
        .find(|clip| (clip.session_start_ms - 5000.0).abs() < 1.0)
        .expect("tail clip");
    assert_span(tail, 5000.0, 7000.0, 1.0);
    assert_position_sec(&events, 4750.0, "B", 1.75);
    let in_gap = reconstruct(&events, 4000.0, "B").expect("deck missing");
    assert!(!in_gap.is_playing, "B is silent inside the deleted range");

    // Edit 3: draw gain 0.3 on deck A over [4000, 6000]. The recorded 0.5 at
    // 4000 is consumed and restored at 6000.
    let spec = lane_spec_for(EditableLane::Gain, &session_core::CLASSIC_3BAND, None, None);
    let events = splice_lane_events(
        &events,
        &spec,
        "A",
        4000.0,
        6000.0,
        &[session_core::LanePoint {
            ms: 4000.0,
            value: 0.3,
        }],
    );
    let mid = reconstruct(&events, 5000.0, "A").expect("deck missing");
    assert!((mid.strip_gain - 0.3).abs() < 1e-6);
    let after = reconstruct(&events, 7000.0, "A").expect("deck missing");
    assert!((after.strip_gain - 0.5).abs() < 1e-6);
    assert_eq!(after.path.as_deref(), Some("/t/a.mp3"));
    // The lane edit must not move the playhead.
    assert_position_sec(&events, 7000.0, "A", 3.0 + 1.0 * 1.5);
}
