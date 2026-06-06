# Architecture

## Overall structure

```mermaid
graph TD
    classDef frontend fill:#3b82f6,stroke:#1d4ed8,color:#fff
    classDef ipc fill:#06b6d4,stroke:#0891b2,color:#fff
    classDef backend fill:#f97316,stroke:#ea580c,color:#fff

    subgraph Frontend ["Frontend (Renderer - Vue 3 + Pinia)"]
        UI["UI Components\n(Vue SFCs)"]:::frontend
        Stores["Pinia Stores\n(decks, settings, library)"]:::frontend
    end

    subgraph IPC ["Tauri IPC Layer"]
        Commands["Tauri Commands\n(commands.rs)"]:::ipc
        Events["Tauri Events\n(track-ended, bands-ready)"]:::ipc
    end

    subgraph Backend ["Backend (Rust)"]
        AppState["AppState\n(Arc<AppAudio> + SessionLogger\n+ session_track_cache\n+ session_snapshots\n+ session_playback_cancel)"]:::backend
        Audio["AppAudio\n(decks, strips, streams)"]:::backend
        Engine["Audio Engine\n(deck.rs, stream.rs, dsp.rs)"]:::backend
        Scheduler["Session Scheduler\n(tokio task, session_playback.rs)"]:::backend
    end

    UI --> Stores
    Stores -- "invoke()" --> Commands
    Events -- "listen()" --> Stores
    Commands --> AppState
    AppState --> Audio
    Audio --> Engine
    Engine -- "emit track-ended" --> Events
    Commands -- "start_session_playback" --> Scheduler
    Scheduler -- "direct AppState access" --> Audio
```

## Audio signal chain (per deck)

```mermaid
graph LR
    classDef io fill:#64748b,stroke:#475569,color:#fff
    classDef deck fill:#3b82f6,stroke:#1d4ed8,color:#fff
    classDef strip fill:#208043,stroke:#166534,color:#fff
    classDef master fill:#d631b0,stroke:#a21caf,color:#fff
    classDef out fill:#f97316,stroke:#ea580c,color:#fff

    File["Audio File\n(.mp3, .flac, .wav)"]:::io
    Decode["Decode + Resample\n(io.rs)"]:::io
    DeckState["DeckState\n(playback position,\ncue/loop logic,\nspectral bands)"]:::deck
    ChannelStrip["ChannelStrip\n(EQ, filter, gain,\nsmoothed volume fader)"]:::strip
    MasterMix["Master Mix\n(limiter, gain,\nmetering)"]:::master
    Outputs["Output Devices\n(main + cue)"]:::out

    File --> Decode
    Decode --> DeckState
    DeckState -- "main_tick()\ncue_tick()" --> ChannelStrip
    ChannelStrip -- "process_main()\nprocess_cue()" --> MasterMix
    MasterMix --> Outputs
```

## Stream routing

```mermaid
flowchart TD
    classDef decision fill:#f97316,stroke:#ea580c,color:#fff
    classDef combined fill:#3b82f6,stroke:#1d4ed8,color:#fff
    classDef separate fill:#208043,stroke:#166534,color:#fff

    Decision{{"Main and cue\non same device?"}}:::decision

    Decision -- "No" --> SepMain["Separate main stream\n(device A, ch offset main)"]:::separate
    Decision -- "No" --> SepCue["Separate cue stream\n(device B, ch offset cue)"]:::separate
    Decision -- "Yes" --> Combined["Combined stream\n(single callback on device A,\nch main_off+0/1 for main,\nch cue_off+0/1 for cue)"]:::combined
```

## CUE state machine

```mermaid
stateDiagram-v2
    classDef stopped fill:#64748b,stroke:#475569,color:#fff
    classDef previewing fill:#06b6d4,stroke:#0891b2,color:#fff
    classDef playing fill:#208043,stroke:#166534,color:#fff

    [*] --> Stopped : track loaded

    Stopped --> Previewing : press_cue (at cue point)
    Stopped --> Stopped : press_cue (away from cue: move cue, no play)
    Stopped --> Playing : toggle_play

    Previewing --> Stopped : release_cue (return to cue)
    Previewing --> Playing : toggle_play (latch to playing)

    Playing --> Stopped : toggle_play
    Playing --> Stopped : press_cue (return to cue)
    Playing --> Playing : loop active (wrap at loop_end)

    class Stopped stopped
    class Previewing previewing
    class Playing playing
```

## Tauri command categories

```mermaid
graph LR
    classDef transport fill:#3b82f6,stroke:#1d4ed8,color:#fff
    classDef loop fill:#208043,stroke:#166534,color:#fff
    classDef mixing fill:#d631b0,stroke:#a21caf,color:#fff
    classDef io fill:#64748b,stroke:#475569,color:#fff
    classDef recording fill:#ef4444,stroke:#b91c1c,color:#fff
    classDef device fill:#f97316,stroke:#ea580c,color:#fff

    subgraph Transport
        play:::transport
        stop:::transport
        press_cue:::transport
        release_cue:::transport
        toggle_play:::transport
        set_cue_and_stop:::transport
        stop_at_cue:::transport
        seek:::transport
    end

    subgraph Loop
        set_loop_in:::loop
        set_loop_out:::loop
        set_loop_active:::loop
        set_loop_region:::loop
        clear_loop_region:::loop
        set_reloop:::loop
    end

    subgraph Mixing
        set_volume:::mixing
        set_eq:::mixing
        set_filter:::mixing
        set_filter_active:::mixing
        set_playback_rate:::mixing
        set_nudge:::mixing
        set_master_gain:::mixing
        set_cue_mix:::mixing
        set_limiter_enabled:::mixing
    end

    subgraph IO
        load_track:::io
        eject_track:::io
        open_file_dialog:::io
        scan_folder:::io
        files_info:::io
        read_track_tags:::io
    end

    subgraph Recording
        start_recording:::recording
        stop_recording:::recording
        save_recording:::recording
        save_bms_only:::recording
        discard_recording:::recording
    end

    subgraph Session
        open_session_dialog:::io
        preload_session:::io
        start_session_playback:::transport
        stop_session_playback:::transport
        render_session_to_file:::recording
    end

    subgraph Device
        list_audio_devices:::device
        set_main_device:::device
        set_cue_device:::device
        set_buffer_size:::device
        set_bpm_range:::device
    end
```

## App modes and transitions

The app has three full-page modes managed by `useAppModeStore`. `App.vue` renders the active view via `v-if` on `appMode.mode`. Transitions are handled centrally in `switchTo()`:

- **Performance ↔ Edit**: non-destructive. Entering Edit stops any playing deck; decks stay loaded.
- **Any → Session**: ejects all decks before switching.
- **Session → Any**: calls `session.exit()` (stops playback, clears the loaded file) then resets the mixer.

The AppBar dropdown triggers `switchTo()` with a confirmation dialog when the transition would be destructive (active playback, loaded tracks, or an open session file).

```mermaid
flowchart TD
    classDef app fill:#64748b,stroke:#475569,color:#fff
    classDef perf fill:#3b82f6,stroke:#1d4ed8,color:#fff
    classDef session fill:#06b6d4,stroke:#0891b2,color:#fff
    classDef rust fill:#f97316,stroke:#ea580c,color:#fff

    App["App.vue\n(AppBar, TopStrip, mode switch)"]:::app
    Perf["Performance.vue\n(decks, mixer, collection)"]:::perf
    Edit["EditView.vue\n(deck E waveform, collection)"]:::perf
    Sess["Session.vue\n(transport, timeline, render)"]:::session
    Timeline["Timeline.vue\n(canvas, click-to-scrub)"]:::session
    Scheduler["Rust session scheduler\n(tokio task)"]:::rust
    Snapshots["SessionSnapshot[]\n(one per event, in AppState)"]:::rust

    App -- "v-if mode='performance'" --> Perf
    App -- "v-if mode='edit'" --> Edit
    App -- "v-if mode='session'" --> Sess
    Sess -- "emit exit" --> App
    Sess --> Timeline
    Timeline -- "emit seek(ms)" --> Sess
    Sess -- "start_session_playback(path, fromMs)" --> Scheduler
    Scheduler -- "binary search" --> Snapshots
    Snapshots -- "snapshot state applied\nto live AppAudio" --> Scheduler
```

## Session playback: scrubbing and snapshot state machine

On session open, `preload_session` decodes all referenced audio files into a persistent cache and builds a `SessionSnapshot` after every event in the session log. Each snapshot captures the complete state of all decks (position, rate, nudge, loop) and all channel strips (gain, EQ, filter).

When `start_session_playback(fromMs)` is called the scheduler:

1. Finds the last snapshot with `elapsed_ms ≤ fromMs` (binary search)
2. Resets all decks and strips
3. Applies the snapshot's strip/master state to the live engine
4. Replays the few events between the snapshot and `fromMs` through the analytical position simulation
5. Sets final deck positions and starts the tokio timer loop for events after `fromMs`

Position simulation rule: any event that changes playback speed or position (`play`, `stop`, `seek`, `set_playback_rate`, `set_nudge`) commits the current analytically-computed position before updating the relevant parameter. This ensures each segment between events is computed with the correct rate and nudge factor.

## .bms file format

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

`elapsed_ms` is milliseconds since the recording started, rounded to 0.1 ms. `startedAt` is an ISO-8601 wall-clock timestamp.

### Event types

| type                                       | relevant fields                                                                                                      | meaning                                                           |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `recording_start`                          | `buffer_size_frames`                                                                                                 | first event; records audio callback size for latency compensation |
| `deck_snapshot`                            | `deck`, `path`, `position_sec`, `cue_point_sec`, `is_playing`, `bpm`, `playback_rate`, `loop_active`, `loop_end_sec` | full deck state at record-start for tracks already loaded         |
| `recording_stop`                           |                                                                                                                      | last event                                                        |
| `load_track`                               | `deck`, `path`, `duration`                                                                                           | track loaded onto deck                                            |
| `play`                                     | `deck`                                                                                                               | deck started playing                                              |
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

### Latency compensation

The live audio engine applies commands on the next callback after they fire. The offline renderer offsets every event by `buffer_size_frames` samples (read from the `recording_start` event, defaulting to 512) so rendered output aligns with the original live recording.

### Offline render

`render_session_to_file` (Tauri command) reads a `.bms`, feeds it through the same DSP signal chain used for live playback (`offline_render.rs`), and writes a 44100 Hz stereo output file. WAV output is 32-bit float; FLAC output is 24-bit (matching the live recording pipeline). The render is deterministic given the same audio files and event log.

### Recording formats

| Setting      | Audio file       | .bms file                               |
| ------------ | ---------------- | --------------------------------------- |
| WAV (16-bit) | 16-bit PCM WAV   | only if "always record .bms" is checked |
| WAV (32-bit) | 32-bit float WAV | only if "always record .bms" is checked |
| FLAC         | 24-bit FLAC      | only if "always record .bms" is checked |
| Session only | none             | always                                  |

`save_bms_only` is used for the Session only path: it discards the audio temp file and writes the session log to the chosen `.bms` path.
