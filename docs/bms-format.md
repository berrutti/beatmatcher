# .bms file format

A `.bms` file is a JSON document (UTF-8, pretty-printed) saved alongside or instead of a recording. The extension stands for Beatmatcher Session.

```json
{
  "version": 2,
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

`version` is a single integer, bumped only when the event vocabulary itself changes, meaning an event type is renamed or replaced or the fields one carries are re-addressed. It is not bumped for a new event type, because a reader that does not know a type ignores it and the rest of the session still replays. The current number lives in one place, `BMS_VERSION` in session-core, so the writer and this document cannot drift apart. The field is required: a document without it is refused rather than guessed at.

An older version is never rejected. Reading a session ports it: every event in a superseded vocabulary is rewritten into the current one at load, before anything interprets it, so playback, the timeline lanes and the lane editor all see one vocabulary and no reader needs to know which version it came from. Porting is a renaming rather than a reinterpretation, and is only possible while every address in the old vocabulary still exists on the mixer the session resolves to, which is what keeps the rewrite from changing how a recording sounds. A ported session is not written back to disk on open: the file changes only when it is saved, and it is then stamped with the version it now contains.

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
| `set_xfader_assign`                        | `deck`, `assign`                                                                                                     | which crossfader bus a channel is on                              |

## Mixer parameters

Every mixer parameter is one `set_param` event addressed as **deck / slot / param**. `slot` is the position in the channel strip, not the unit filling it, so replacing the unit in a slot keeps existing automation pointing at the same place. Omitting `deck` addresses master scope.

| slot     | param                  | scope  | value                                |
| -------- | ---------------------- | ------ | ------------------------------------ |
| `fader`  | `gain`                 | deck   | 0-1, channel fader                   |
| `eq`     | `low` / `mid` / `high` | deck   | dB, -26 to +6                        |
| `filter` | `value`                | deck   | -1 to +1, negative LPF, positive HPF |
| `filter` | `active`               | deck   | 0 or 1, filter on/off                |
| `gain`   | `gain`                 | master | 0-1, master output level             |
| `xfader` | `position`             | master | -1 to +1, -1 full A, +1 full B       |

```json
{ "elapsed_ms": 4200.0, "type": "set_param", "deck": "A", "slot": "eq", "param": "low", "value": -6.0 }
{ "elapsed_ms": 4900.0, "type": "set_param", "slot": "gain", "param": "gain", "value": 0.7943 }
```

The table above is the `classic-3band-v2` manifest. The slot and param set, and the range each value is read in, come from whichever manifest the `mixer` header names: `isolator-3band` uses the same addresses but reads `eq` bands as 0-1 kill amounts rather than dB. This is why the header carries a hash, and why a value cannot be interpreted without resolving the manifest first. A `set_param` naming a slot or param that manifest does not have is ignored, so the rest of the session still replays.

## Crossfader

`xfader/position` is master scope, but the gain it implies is per channel, because each channel decides whether it listens to it. That assignment is categorical rather than a number, so it is its own event rather than a `set_param`:

```json
{ "elapsed_ms": 3000.0, "type": "set_xfader_assign", "deck": "A", "assign": "a" }
{ "elapsed_ms": 4100.0, "type": "set_param", "slot": "xfader", "param": "position", "value": -1.0 }
```

`assign` is `a`, `b`, or `thru`. An unrecognized value reads as `thru`, so a session written by a newer build loses the assignment rather than failing to load. `thru` is the default and multiplies the channel by exactly 1 wherever the crossfader sits, which is why a session that never mentions the crossfader is unaffected by it.

The curve is constant power: both buses sit at -3 dB with the fader centred, and the ends are exactly 0 and 1 rather than merely close, so a fully cut channel is silent rather than 140 dB down.

The crossfader arrived in the `-v2` manifests. The `classic-3band` and `isolator-3band` manifests are frozen at their original shape rather than gaining the slot, because the header hash covers master slots and adding one would have refused every session recorded before it. A pre-crossfader session therefore resolves a pre-crossfader mixer and renders exactly as it always did.

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
