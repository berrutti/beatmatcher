# .bms file format

A `.bms` file is a JSON document (UTF-8, pretty-printed) saved alongside or instead of a recording. The extension stands for Beatmatcher Session.

```json
{
  "version": 1,
  "startedAt": "2026-06-06T14:00:00Z",
  "events": [
    { "elapsed_ms": 0,      "type": "recording_start", "buffer_size_frames": 512 },
    { "elapsed_ms": 0,      "type": "deck_snapshot", "deck": "A", "path": "/...", "position_sec": 12.3, "cue_point_sec": 0, "is_playing": false, "bpm": 128.0, "playback_rate": 1.0, "loop_active": false, "loop_end_sec": 0 },
    { "elapsed_ms": 1234.5, "type": "play",     "deck": "A" },
    { "elapsed_ms": 5678.0, "type": "load_track","deck": "B", "path": "/..." },
    ...
    { "elapsed_ms": 3600000, "type": "recording_stop" }
  ]
}
```

`elapsed_ms` is milliseconds since the recording started, at full f64 precision. `startedAt` is an ISO-8601 wall-clock timestamp.

## Event types

| type                                       | relevant fields                                                                                                      | meaning                                                           |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `recording_start`                          | `buffer_size_frames`                                                                                                 | first event; records audio callback size for latency compensation |
| `deck_snapshot`                            | `deck`, `path`, `position_sec`, `cue_point_sec`, `is_playing`, `bpm`, `playback_rate`, `loop_active`, `loop_end_sec` | full deck state at record-start for tracks already loaded         |
| `recording_stop`                           |                                                                                                                      | last event                                                        |
| `load_track`                               | `deck`, `path`, `duration`                                                                                           | track loaded onto deck                                            |
| `play`                                     | `deck`, `sec` (optional; written by clip edits, never by the recorder)                                               | deck started playing, optionally from an explicit position        |
| `stop`                                     | `deck`                                                                                                               | deck stopped                                                      |
| `seek`                                     | `deck`, `sec`                                                                                                        | playhead jumped                                                   |
| `set_cue` / `stop_at_cue`                  | `deck`, `cue_sec`                                                                                                    | cue point set or jump-to-cue                                      |
| `set_playback_rate`                        | `deck`, `rate`                                                                                                       | pitch/rate changed                                                |
| `set_nudge`                                | `deck`, `percent`                                                                                                    | nudge started or released                                         |
| `loop_in` / `loop_out` / `set_loop_region` | `deck`, `start_sec`, `end_sec`                                                                                       | loop points changed                                               |
| `set_loop_active`                          | `deck`, `active`                                                                                                     | loop toggled                                                      |
| `set_volume`                               | `deck`, `value`                                                                                                      | channel fader                                                     |
| `set_eq`                                   | `deck`, `band` (`low`/`mid`/`high`), `db`                                                                            | EQ band                                                           |
| `set_filter`                               | `deck`, `value`                                                                                                      | filter frequency (0-1)                                            |
| `set_filter_active`                        | `deck`, `active`                                                                                                     | filter on/off                                                     |
| `set_master_gain`                          | `gain`                                                                                                               | master output level                                               |

## Latency compensation

The live audio engine applies commands on the next callback after they fire. The offline renderer offsets every event by `buffer_size_frames` samples (read from the `recording_start` event, defaulting to 512) so rendered output aligns with the original live recording.

## Offline render

`render_session_to_file` (Tauri command) reads a `.bms`, feeds it through the same DSP signal chain used for live playback (`offline_render.rs`), and writes a 44100 Hz stereo output file. WAV output is 32-bit float; FLAC output is 24-bit (matching the live recording pipeline). The render is deterministic given the same audio files and event log.

## Recording formats

| Setting      | Audio file       | .bms file                               |
| ------------ | ---------------- | --------------------------------------- |
| WAV (16-bit) | 16-bit PCM WAV   | only if "always record .bms" is checked |
| WAV (32-bit) | 32-bit float WAV | only if "always record .bms" is checked |
| FLAC         | 24-bit FLAC      | only if "always record .bms" is checked |
| Session only | none             | always                                  |

`save_bms_only` is used for the Session only path: it discards the audio temp file and writes the session log to the chosen `.bms` path.
