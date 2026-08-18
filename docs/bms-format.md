# .bms file format

A `.bms` file is a JSON document (UTF-8, pretty-printed) saved alongside or instead of a recording. The extension stands for Beatmatcher Session.

```json
{
  "version": 2,
  "startedAt": "2026-06-06T14:00:00Z",
  "mixer": { "id": "classic-3band", "hash": "a1b2c3d4e5f60718" },
  "events": [
    { "elapsed_ms": 0,      "frame": 0, "type": "recording_start", "buffer_size_frames": 512, "sample_rate": 48000 },
    { "elapsed_ms": 0,      "type": "deck_snapshot", "deck": "A", "path": "/...", "position_sec": 12.3, "cue_point_sec": 0, "is_playing": false, "bpm": 128.0, "playback_rate": 1.0, "loop_active": false, "loop_end_sec": 0 },
    { "elapsed_ms": 1234.5, "type": "play",     "deck": "A" },
    { "elapsed_ms": 5678.0, "type": "load_track","deck": "B", "path": "/..." },
    ...
    { "elapsed_ms": 3600000, "type": "recording_stop" }
  ]
}
```

`elapsed_ms` is milliseconds since the recording started, at full f64 precision. `startedAt` is an ISO-8601 wall-clock timestamp. `frame` is output frames since the first one the recorder captured, and it counts from a different origin than `elapsed_ms`.

`version` is a required integer, bumped only when the event vocabulary changes. A new event type does not bump it, because a reader that does not know a type ignores it. The number lives in `BMS_VERSION` in session-core.

An older version is never rejected. Reading a session rewrites its events into the current vocabulary before anything interprets them, and the file on disk changes only when it is saved.

`mixer` names the mixer manifest the session was played on. `hash` covers everything that changes what a `set_param` means (slot order, unit ids, param ids, ranges, defaults, steps, dead zones) and excludes display labels. Rendering refuses a session whose mixer this build does not have or has since changed. A file with no `mixer` field replays on the classic mixer.

## Event types

| type                     | relevant fields                                                                                                      | meaning                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `recording_start`        | `buffer_size_frames`, `sample_rate`                                                                                  | first event; audio callback size and the rate `frame` counts in |
| `deck_snapshot`          | `deck`, `path`, `position_sec`, `cue_point_sec`, `is_playing`, `bpm`, `playback_rate`, `loop_active`, `loop_end_sec` | full deck state at record-start for tracks already loaded       |
| `recording_stop`         |                                                                                                                      | last event                                                      |
| `load_track`             | `deck`, `path`, `duration`                                                                                           | track loaded onto deck                                          |
| `play`                   | `deck`, `sec` (optional; written by clip edits, never by the recorder)                                               | deck started playing, optionally from an explicit position      |
| `stop`                   | `deck`                                                                                                               | deck stopped                                                    |
| `seek`                   | `deck`, `sec`                                                                                                        | playhead jumped                                                 |
| `stopped_at_cue`         | `deck`, `cue_sec`                                                                                                    | CUE pressed while playing: stops and returns to the cue point   |
| `set_playback_rate`      | `deck`, `rate`                                                                                                       | pitch/rate changed                                              |
| `set_nudge`              | `deck`, `percent`                                                                                                    | nudge started or released                                       |
| `loop_in` / `loop_out`   | `deck`, `start_sec`, `end_sec`                                                                                       | loop points changed                                             |
| `exit_loop` / `reloop`   | `deck`                                                                                                               | loop left, or re-entered from its start                         |
| `jog`                    | `deck`, `ticks`                                                                                                      | jog wheel moved, see below                                      |
| `set_jog_rotation_speed` | `speed`                                                                                                              | the rpm one jog tick stands for                                 |
| `set_param`              | `deck` (omitted at master scope), `slot`, `param`, `value`                                                           | any mixer parameter, see below                                  |
| `set_xfader_assign`      | `deck`, `assign`                                                                                                     | which crossfader bus a channel is on                            |
| `set_fader_curve`        | `curve`                                                                                                              | the taper every channel fader runs on                           |

## Mixer parameters

Every mixer parameter is one `set_param` event addressed as **deck / slot / param**. `slot` is the position in the channel strip, so swapping the unit in a slot keeps existing automation pointing at the same place. Omitting `deck` addresses master scope.

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

The table above is the `classic-3band-v2` manifest. Addresses and ranges come from whichever manifest the `mixer` header names, so `isolator-3band` reads `eq` bands as 0-1 kill amounts instead of dB. A `set_param` naming an address that manifest lacks is ignored.

## Crossfader

`xfader/position` is master scope, and each channel decides whether it listens to it. That assignment is categorical, so it rides its own event:

```json
{ "elapsed_ms": 3000.0, "type": "set_xfader_assign", "deck": "A", "assign": "a" }
{ "elapsed_ms": 4100.0, "type": "set_param", "slot": "xfader", "param": "position", "value": -1.0 }
```

`assign` is `a`, `b`, or `thru`. An unrecognized value reads as `thru`, the default, which multiplies the channel by exactly 1 wherever the crossfader sits.

The curve is constant power, with both buses at -3 dB when centred and the ends at exactly 0 and 1.

The crossfader arrived in the `-v2` manifests. `classic-3band` and `isolator-3band` stay frozen at their original shape, because the header hash covers master slots and adding one would have refused every session recorded before it.

## Channel fader curve

`fader/gain` records the throw of the fader. How that throw maps to gain is the curve, one setting for every channel at once, and like `set_xfader_assign` it is a named alternative rather than a number, so it rides its own event:

```json
{ "elapsed_ms": 0.0, "type": "set_fader_curve", "curve": "exponential" }
```

`curve` is `exponential`, `linear` or `logarithmic`. An unrecognized value reads as `linear`, which is also the default.

Recording writes the curve once at `recording_start`, because nothing else in the session would say which taper the fader moves were played through. All three curves hold both ends of the throw exactly, so the CUE sheet's audibility test is unaffected by the choice.

## Mixer state at record start

`recording_start` is followed by a `set_param` for every strip param that is not at its manifest default, one per deck. A knob moved before recording began is otherwise lost, and a reader replays the manifest default in its place: an engaged filter read as bypassed, so a whole sweep was silent in the render. Params already at their default are skipped, for the same reason a `thru` crossfader assign is.

## Jog wheel

`jog` records the wheel's own input, because its effect is computed per audio block and is never known on the thread that logs. `ticks` already carries the shift scale, so a replay needs no shift state.

```json
{ "elapsed_ms": 61234.5, "type": "set_jog_rotation_speed", "speed": "rpm33" }
{ "elapsed_ms": 61240.0, "type": "jog", "deck": "A", "ticks": 6 }
```

`speed` is `rpm33` or `rpm45`, and an unrecognized value reads as `rpm33`, the default. It is stamped at `recording_start` because a revolution covers 60/rpm seconds of audio, so nothing else would say how far a scrub travelled.

A tick is worth `0.002 s` of audio at 33. A paused deck scrubs that distance and a playing one bends by a hundredth of it. The engine spreads the travel over a 40 ms filter settle, which changes when it arrives and never how much, so a reader that wants only the total can ignore the filter.

## Command timing

Nothing is compensated. `elapsed_ms` is stamped in Rust when the command arrives, so the hop from the frontend is already outside it, and the renderer applies each command exactly where the live engine did.

Where that is follows from block rendering: the audio callback renders a whole block under one deck lock, so a command arriving part-way through cannot alter frames already written and takes effect on the next callback. A recorded event carries `frame`, read under the same lock as the mutation, and the renderer dispatches there verbatim. An event with no `frame` is one nothing performed, a synthesized edit or a session older than the stamp, and dispatches at `elapsed_ms`. `frame` counts at the recording's `sample_rate`, so a render at another rate scales it.

The delay is between zero and one buffer, not a fixed offset: measured at 35 frames on a 128-frame buffer. Adding a fixed buffer to every event puts every deck out, which is why the frame is recorded rather than inferred.

## Offline render

`render_session_to_file` (Tauri command) reads a `.bms`, feeds it through the same DSP signal chain used for live playback (`offline_render.rs`), and writes a 44100 Hz stereo output file. WAV output is 32-bit float; FLAC output is 24-bit (matching the live recording pipeline). The render is deterministic given the same audio files and event log.

## Recording formats

| Setting      | Audio file       |
| ------------ | ---------------- |
| WAV (16-bit) | 16-bit PCM WAV   |
| WAV (32-bit) | 32-bit float WAV |
| FLAC         | 24-bit FLAC      |
| Session only | none             |

The three audio settings write a `.bms` alongside the audio when "always record .bms" is checked. Session only always writes one, through `save_bms_only`, which discards the audio temp file and writes the session log to the chosen path.
