use crate::audio::Deck;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeckSyncPayload {
    pub(crate) is_playing: bool,
    pub(crate) is_cueing: bool,
    pub(crate) cue_point_sec: f64,
    pub(crate) position_sec: f64,
    pub(crate) loop_active: bool,
    pub(crate) loop_region_cleared: bool,
    // A controller press never sees `LoopOutResult`, so the region it just
    // defined has to arrive with the state rather than as a return value.
    pub(crate) loop_region: Option<LoopRegionPayload>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoopRegionPayload {
    start_sec: f64,
    end_sec: f64,
    beats: i64,
}

impl DeckSyncPayload {
    pub(crate) fn from_deck(deck_state: &Deck, loop_region_cleared: bool) -> Self {
        let sr = deck_state.device_sample_rate as f64;
        Self {
            is_playing: deck_state.is_playing,
            is_cueing: deck_state.is_cueing,
            cue_point_sec: if sr > 0.0 {
                deck_state.cue_point / sr
            } else {
                0.0
            },
            position_sec: deck_state.position_sec(),
            loop_active: deck_state.loop_active,
            loop_region_cleared,
            loop_region: loop_region_of(deck_state, sr),
        }
    }
}

/// The loop's start is the cue point, which `loop_in` and `set_loop_region` both
/// write, so a defined region is `loop_end` above it rather than a pair.
fn loop_region_of(deck_state: &Deck, sr: f64) -> Option<LoopRegionPayload> {
    if sr <= 0.0 || deck_state.loop_end <= deck_state.cue_point {
        return None;
    }
    let start_sec = deck_state.cue_point / sr;
    let end_sec = deck_state.loop_end / sr;
    Some(LoopRegionPayload {
        start_sec,
        end_sec,
        beats: match deck_state.bpm {
            Some(bpm) => ((end_sec - start_sec) * bpm / 60.0).round() as i64,
            None => 0,
        },
    })
}
