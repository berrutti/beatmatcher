# .bms file format

A `.bms` file is a JSON document (UTF-8, pretty-printed) saved alongside or instead of a recording. The extension stands for Beatmatcher Session.

```json
{
  "version": 1,
  "startedAt": "2026-06-06T14:00:00Z",
  "mixer": { "id": "classic-3band", "hash": "a1b2c3d4e5f60718" },
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

`mixer` names the mixer manifest the session was played on. `hash` covers everything that changes what a `set_param` event means (slot order, unit ids, param ids, ranges, defaults, steps, dead zones), but not display labels, so renaming a knob does not invalidate existing sessions. Rendering refuses a session whose mixer this build does not have, or whose mixer has changed shape since, rather than producing output that silently differs from the recording. Sessions written before manifests existed have no `mixer` field and replay on the classic mixer.

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
| `set_param`                                | `deck` (omitted at master scope), `slot`, `param`, `value`                                                           | any mixer parameter, see below                                    |

## Mixer parameters

Every mixer parameter is one `set_param` event addressed as **deck / slot / param**. `slot` is the position in the channel strip, not the unit filling it, so replacing the unit in a slot keeps existing automation pointing at the same place. Omitting `deck` addresses master scope.

| slot     | param                  | scope  | value                                |
| -------- | ---------------------- | ------ | ------------------------------------ |
| `fader`  | `gain`                 | deck   | 0-1, channel fader                   |
| `eq`     | `low` / `mid` / `high` | deck   | dB, -26 to +6                        |
| `filter` | `value`                | deck   | -1 to +1, negative LPF, positive HPF |
| `filter` | `active`               | deck   | 0 or 1, filter on/off                |
| `gain`   | `gain`                 | master | 0-1, master output level             |

```json
{ "elapsed_ms": 4200.0, "type": "set_param", "deck": "A", "slot": "eq", "param": "low", "value": -6.0 }
{ "elapsed_ms": 4900.0, "type": "set_param", "slot": "gain", "param": "gain", "value": 0.7943 }
```

The table above is the `classic-3band` manifest. The slot and param set, and the range each value is read in, come from whichever manifest the `mixer` header names: `isolator-3band` uses the same addresses but reads `eq` bands as 0-1 kill amounts rather than dB. This is why the header carries a hash, and why a value cannot be interpreted without resolving the manifest first. A `set_param` naming a slot or param that manifest does not have is ignored, so the rest of the session still replays.

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
