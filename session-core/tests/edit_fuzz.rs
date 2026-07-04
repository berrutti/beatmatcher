// Deterministic randomized stress for the clip edit ops; every failure message
// carries its seed, so a failing case replays exactly.

use session_core::{
    blocks_for_deck, build_clips, delete_transport_ranges, event_sim_order, move_transport_block,
    trim_transport_block, DeleteRange, Edge, SessionEvent,
};

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }

    fn pick(&mut self, count: usize) -> usize {
        (self.next_u64() % count as u64) as usize
    }
}

fn make_event(at_ms: f64, event_type: &str, deck: &str) -> SessionEvent {
    SessionEvent::at(at_ms, event_type, deck)
}

fn random_session(rng: &mut Rng) -> Vec<SessionEvent> {
    let mut events: Vec<SessionEvent> = Vec::new();
    for deck in ["A", "B"] {
        let mut at_ms = rng.range(0.0, 500.0);
        events.push(SessionEvent {
            path: Some(format!("/t/{deck}.mp3")),
            ..make_event(at_ms, "load_track", deck)
        });
        let mut playing = false;
        while at_ms < 60_000.0 {
            at_ms += rng.range(300.0, 8000.0);
            match rng.pick(6) {
                0 | 1 => {
                    if playing {
                        events.push(make_event(at_ms, "stop", deck));
                    } else {
                        events.push(make_event(at_ms, "play", deck));
                    }
                    playing = !playing;
                }
                2 => events.push(SessionEvent {
                    sec: Some(rng.range(0.0, 120.0)),
                    ..make_event(at_ms, "seek", deck)
                }),
                3 => events.push(SessionEvent {
                    rate: Some(rng.range(0.92, 1.08)),
                    ..make_event(at_ms, "set_playback_rate", deck)
                }),
                4 => events.push(SessionEvent {
                    gain: Some(rng.range(0.0, 1.0) as f32),
                    ..make_event(at_ms, "set_volume", deck)
                }),
                _ => {
                    if playing {
                        let start_sec = rng.range(0.0, 60.0);
                        events.push(SessionEvent {
                            start_sec: Some(start_sec),
                            end_sec: Some(start_sec + rng.range(0.5, 4.0)),
                            ..make_event(at_ms, "loop_out", deck)
                        });
                        at_ms += rng.range(1000.0, 6000.0);
                        events.push(make_event(at_ms, "exit_loop", deck));
                    }
                }
            }
        }
        if playing {
            at_ms += rng.range(300.0, 3000.0);
            events.push(make_event(at_ms, "stop", deck));
        }
    }
    events.sort_by(event_sim_order);
    events
}

fn apply_random_edit(rng: &mut Rng, events: &[SessionEvent]) -> (Vec<SessionEvent>, String) {
    let clips = build_clips(events).clips;
    let deck = if rng.pick(2) == 0 { "A" } else { "B" };
    let blocks = blocks_for_deck(&clips, deck);
    if blocks.is_empty() {
        return (events.to_vec(), format!("no blocks on {deck}, no edit"));
    }
    let block = blocks[rng.pick(blocks.len())].clone();
    match rng.pick(3) {
        0 => {
            let delta = rng.range(-10_000.0, 10_000.0);
            let description = format!(
                "move {deck} [{:.0}, {:.0}] by {delta:.0}",
                block.start_ms, block.end_ms
            );
            (
                move_transport_block(events, &clips, &block, delta).events,
                description,
            )
        }
        1 => {
            let edge = if rng.pick(2) == 0 {
                Edge::Start
            } else {
                Edge::End
            };
            let new_ms = rng.range((block.start_ms - 8000.0).max(0.0), block.end_ms + 8000.0);
            let description = format!(
                "trim {deck} [{:.0}, {:.0}] {edge:?} to {new_ms:.0}",
                block.start_ms, block.end_ms
            );
            (
                trim_transport_block(events, &clips, &block, edge, new_ms).events,
                description,
            )
        }
        _ => {
            let start_ms = rng.range(0.0, 70_000.0);
            let end_ms = start_ms + rng.range(100.0, 20_000.0);
            let description = format!("delete {deck} range [{start_ms:.0}, {end_ms:.0}]");
            let range = DeleteRange {
                deck: deck.to_string(),
                start_ms,
                end_ms,
            };
            (
                delete_transport_ranges(events, &clips, &[range]),
                description,
            )
        }
    }
}

fn assert_invariants(events: &[SessionEvent], context: &str) {
    for pair in events.windows(2) {
        assert_ne!(
            event_sim_order(&pair[0], &pair[1]),
            std::cmp::Ordering::Greater,
            "{context}: edited events are not sorted ({} at {} before {} at {})",
            pair[0].event_type,
            pair[0].elapsed_ms,
            pair[1].event_type,
            pair[1].elapsed_ms,
        );
    }

    let clips = build_clips(events).clips;
    for clip in &clips {
        assert!(
            clip.session_end_ms >= clip.session_start_ms,
            "{context}: negative-length clip {clip:?}"
        );
    }
    for deck in ["A", "B"] {
        let blocks = blocks_for_deck(&clips, deck);
        for pair in blocks.windows(2) {
            assert!(
                pair[0].end_ms <= pair[1].start_ms + 1.0,
                "{context}: overlapping blocks on {deck}: [{:.1}, {:.1}] and [{:.1}, {:.1}]",
                pair[0].start_ms,
                pair[0].end_ms,
                pair[1].start_ms,
                pair[1].end_ms,
            );
        }
    }
}

#[test]
fn random_edits_uphold_structural_invariants() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut events = random_session(&mut rng);
        assert_invariants(&events, &format!("seed {seed}: initial session"));
        for step in 0..3 {
            let (edited, description) = apply_random_edit(&mut rng, &events);
            assert_invariants(&edited, &format!("seed {seed} step {step}: {description}"));
            events = edited;
        }
    }
}

// The batch range delete relies on this to re-locate targets between deletes.
#[test]
fn whole_block_delete_leaves_other_blocks_identical() {
    for seed in 1..=150u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        let events = random_session(&mut rng);
        let clips = build_clips(&events).clips;
        for deck in ["A", "B"] {
            let blocks = blocks_for_deck(&clips, deck);
            if blocks.len() < 2 {
                continue;
            }
            let target_index = rng.pick(blocks.len());
            let target = blocks[target_index].clone();
            let edited = session_core::delete_block_range(
                &events,
                &clips,
                &target,
                target.start_ms,
                target.end_ms,
            );
            let rebuilt = build_clips(&edited).clips;
            let rebuilt_blocks = blocks_for_deck(&rebuilt, deck);
            for (block_index, original) in blocks.iter().enumerate() {
                if block_index == target_index {
                    continue;
                }
                let survived = rebuilt_blocks.iter().any(|candidate| {
                    (candidate.start_ms - original.start_ms).abs() < 1.0
                        && (candidate.end_ms - original.end_ms).abs() < 1.0
                        && (candidate.track_start_sec - original.track_start_sec).abs() < 1e-6
                });
                assert!(
                    survived,
                    "seed {seed}: deleting {deck} block [{:.1}, {:.1}] altered block \
                     [{:.1}, {:.1}] (track_start {:.4}); rebuilt: {rebuilt_blocks:?}",
                    target.start_ms,
                    target.end_ms,
                    original.start_ms,
                    original.end_ms,
                    original.track_start_sec,
                );
            }
        }
    }
}
