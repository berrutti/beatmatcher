use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

const CLIENT_NAME: &str = "Beatmatcher";
const CONNECTION_NAME: &str = "Beatmatcher input";
const MONITOR_CAPACITY: usize = 512;
const MONITOR_FLUSH_MS: u64 = 50;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiMessage {
    port: String,
    timestamp_us: u64,
    data: Vec<u8>,
}

struct Monitor {
    buffer: Mutex<VecDeque<MidiMessage>>,
    enabled: AtomicBool,
}

impl Monitor {
    fn new() -> Self {
        Self {
            buffer: Mutex::new(VecDeque::new()),
            enabled: AtomicBool::new(false),
        }
    }

    fn push(&self, message: MidiMessage) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if buffer.len() == MONITOR_CAPACITY {
            buffer.pop_front();
        }
        buffer.push_back(message);
    }

    fn drain(&self) -> Vec<MidiMessage> {
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        buffer.drain(..).collect()
    }
}

enum Request {
    Connect(String, Sender<Result<(), String>>),
    Disconnect,
    Send(Vec<u8>),
}

type Dispatch = Arc<dyn Fn(&[u8]) + Send + Sync>;
type DispatchSlot = Arc<Mutex<Option<Dispatch>>>;

pub struct MidiState {
    requests: Mutex<Sender<Request>>,
    monitor: Arc<Monitor>,
    dispatch: DispatchSlot,
    connected: Mutex<Option<String>>,
    profile: Profile,
    memory: Mutex<ControlMemory>,
}

impl MidiState {
    // The connection is owned by one thread and never shared, so nothing has to
    // reason about whether a platform's midir handle can cross threads.
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let monitor = Arc::new(Monitor::new());
        let dispatch: DispatchSlot = Arc::new(Mutex::new(None));
        let owned = Arc::clone(&monitor);
        let owned_dispatch = Arc::clone(&dispatch);
        std::thread::spawn(move || serve(receiver, owned, owned_dispatch));
        Self {
            requests: Mutex::new(sender),
            monitor,
            dispatch,
            connected: Mutex::new(None),
            profile: ddj_flx6(),
            memory: Mutex::new(ControlMemory::default()),
        }
    }

    pub(crate) fn clear_control_memory(&self) {
        self.memory
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    fn send(&self, request: Request) {
        let sender = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _ = sender.send(request);
    }
}

fn serve(requests: Receiver<Request>, monitor: Arc<Monitor>, dispatch: DispatchSlot) {
    let mut input: Option<midir::MidiInputConnection<()>> = None;
    let mut output: Option<midir::MidiOutputConnection> = None;
    while let Ok(request) = requests.recv() {
        match request {
            Request::Send(bytes) => {
                if let Some(port) = output.as_mut() {
                    let _ = port.send(&bytes);
                }
            }
            Request::Disconnect => {
                drop(input.take());
                drop(output.take());
            }
            Request::Connect(port_name, reply) => {
                // A controller that allows one client at a time refuses a second
                // connection, so the open port must close before the next opens.
                drop(input.take());
                drop(output.take());
                let opened = connect(&port_name, Arc::clone(&monitor), Arc::clone(&dispatch));
                let _ = reply.send(match opened {
                    Ok(open) => {
                        input = Some(open);
                        Ok(())
                    }
                    Err(error) => Err(error),
                });
                // Feedback is a bonus, not a requirement: a controller with no
                // matching output still works, it just cannot light its buttons.
                output = connect_output(&port_name);
            }
        }
    }
    // The bindings exist only for their Drop: dropping them closes the ports.
    drop(input);
    drop(output);
}

fn connect_output(port_name: &str) -> Option<midir::MidiOutputConnection> {
    let output = midir::MidiOutput::new(CLIENT_NAME).ok()?;
    let ports = output.ports();
    let port = ports
        .iter()
        .find(|port| output.port_name(port).is_ok_and(|name| name == port_name))?;
    output.connect(port, CONNECTION_NAME).ok()
}

fn connect(
    port_name: &str,
    monitor: Arc<Monitor>,
    dispatch: DispatchSlot,
) -> Result<midir::MidiInputConnection<()>, String> {
    let mut input = midir::MidiInput::new(CLIENT_NAME).map_err(|error| error.to_string())?;
    input.ignore(midir::Ignore::None);
    let ports = input.ports();
    let port = ports
        .iter()
        .find(|port| input.port_name(port).is_ok_and(|name| name == port_name))
        .ok_or_else(|| format!("no MIDI input named '{port_name}'"))?;
    let source = port_name.to_string();
    input
        .connect(
            port,
            CONNECTION_NAME,
            move |timestamp_us, data, _| {
                monitor.push(MidiMessage {
                    port: source.clone(),
                    timestamp_us,
                    data: data.to_vec(),
                });
                // Cloned out so the slot is not held while the handler takes
                // engine locks.
                let handler = dispatch
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                if let Some(handler) = handler {
                    handler(data);
                }
            },
            (),
        )
        .map_err(|error| error.to_string())
}

/// The MIDI thread reaches the rest of the app through this one closure and
/// nothing else, so mapped input cannot reach device or buffer configuration.
/// Those rebuild the streams and have to stay on the main thread; see
/// `stream.rs` and `stream_commands_must_stay_synchronous`.
pub fn set_dispatch(state: &MidiState, dispatch: Dispatch) {
    *state
        .dispatch
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(dispatch);
}

const CONTROL_CHANGE: u8 = 0xB0;
const NOTE_ON: u8 = 0x90;
// High-resolution control change puts the low half on the controller 32 above
// the high half's.
const LSB_OFFSET: u8 = 32;
const MAX_CONTROLLER: u8 = 127;
const SEVEN_BIT_MAX: f64 = 127.0;
const FOURTEEN_BIT_MAX: f64 = 16383.0;
const RELATIVE_CENTRE: i32 = 64;
const SEVEN_BIT_WRAP: i32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resolution {
    SevenBit,
    FourteenBit,
    // Read off the platter: a steady turn holds one value just off
    // `RELATIVE_CENTRE` and a faster one sits further off, so it reports speed.
    CentreDelta,
    // The browse encoder: 1 or 127, one detent either way.
    SignedStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Half {
    Msb,
    Lsb,
}

/// Distinct from `Source` because a high-resolution control declares one source
/// but arrives on two addresses, and dispatch has to find it by either.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    ControlChange { channel: u8, controller: u8 },
    Note { channel: u8, note: u8 },
}

/// A high-resolution control names only its high half; the low half is implied
/// at `LSB_OFFSET` above it.
enum Source {
    ControlChange {
        channel: u8,
        controller: u8,
        resolution: Resolution,
    },
    Note {
        channel: u8,
        note: u8,
    },
}

impl Source {
    fn keys(&self) -> Result<Vec<(Key, Half)>, String> {
        match *self {
            Source::Note { channel, note } => Ok(vec![(Key::Note { channel, note }, Half::Msb)]),
            Source::ControlChange {
                channel,
                controller,
                resolution,
            } => {
                let high = (
                    Key::ControlChange {
                        channel,
                        controller,
                    },
                    Half::Msb,
                );
                if resolution != Resolution::FourteenBit {
                    return Ok(vec![high]);
                }
                let low = controller
                    .checked_add(LSB_OFFSET)
                    .filter(|low| *low <= MAX_CONTROLLER)
                    .ok_or_else(|| {
                        format!("cc {controller} is too high to carry a low half at +{LSB_OFFSET}")
                    })?;
                Ok(vec![
                    high,
                    (
                        Key::ControlChange {
                            channel,
                            controller: low,
                        },
                        Half::Lsb,
                    ),
                ])
            }
        }
    }

    fn resolution(&self) -> Resolution {
        match *self {
            Source::ControlChange { resolution, .. } => resolution,
            Source::Note { .. } => Resolution::SevenBit,
        }
    }
}

enum Action {
    DeckParam {
        deck: String,
        slot: String,
        param: String,
    },
    CueToggle {
        deck: String,
    },
    PlayToggle {
        deck: String,
    },
    TransportCue {
        deck: String,
    },
    LoopIn {
        deck: String,
    },
    LoopOut {
        deck: String,
    },
    LoopExitOrReloop {
        deck: String,
    },
    TempoFader {
        deck: String,
    },
    Jog {
        deck: String,
    },
    Browse,
    // Master scope, so it names no deck and takes its range from the master
    // descriptor rather than a strip's.
    XfaderPosition,
}

struct Binding {
    source: Source,
    action: Action,
}

pub struct Profile {
    bindings: Vec<Binding>,
    by_key: HashMap<Key, (usize, Half)>,
    cue_keys: HashMap<String, Key>,
}

impl Profile {
    /// Refuses a colliding profile rather than letting the last binding win,
    /// because the symptom is the wrong deck's control moving, which reads as
    /// broken hardware rather than a broken mapping.
    fn new(bindings: Vec<Binding>) -> Result<Self, String> {
        let mut by_key = HashMap::new();
        let mut cue_keys = HashMap::new();
        for (index, binding) in bindings.iter().enumerate() {
            let keys = binding.source.keys()?;
            if let Action::CueToggle { deck } = &binding.action {
                if let Some((key, _)) = keys.first() {
                    cue_keys.insert(deck.clone(), *key);
                }
            }
            for (key, half) in keys {
                if by_key.insert(key, (index, half)).is_some() {
                    return Err(format!("two bindings share {key:?}"));
                }
            }
        }
        Ok(Self {
            bindings,
            by_key,
            cue_keys,
        })
    }

    fn resolve(&self, key: Key) -> Option<(usize, &Binding, Half)> {
        let &(index, half) = self.by_key.get(&key)?;
        Some((index, &self.bindings[index], half))
    }
}

fn deck_param(channel: u8, controller: u8, deck: &str, slot: &str, param: &str) -> Binding {
    Binding {
        source: Source::ControlChange {
            channel,
            controller,
            resolution: Resolution::FourteenBit,
        },
        action: Action::DeckParam {
            deck: deck.to_string(),
            slot: slot.to_string(),
            param: param.to_string(),
        },
    }
}

fn xfader(channel: u8, controller: u8) -> Binding {
    Binding {
        source: Source::ControlChange {
            channel,
            controller,
            resolution: Resolution::FourteenBit,
        },
        action: Action::XfaderPosition,
    }
}

fn cue_toggle(channel: u8, note: u8, deck: &str) -> Binding {
    Binding {
        source: Source::Note { channel, note },
        action: Action::CueToggle {
            deck: deck.to_string(),
        },
    }
}

fn play_toggle(channel: u8, note: u8, deck: &str) -> Binding {
    Binding {
        source: Source::Note { channel, note },
        action: Action::PlayToggle {
            deck: deck.to_string(),
        },
    }
}

fn transport_cue(channel: u8, note: u8, deck: &str) -> Binding {
    Binding {
        source: Source::Note { channel, note },
        action: Action::TransportCue {
            deck: deck.to_string(),
        },
    }
}

fn deck_note(channel: u8, note: u8, action: Action) -> Binding {
    Binding {
        source: Source::Note { channel, note },
        action,
    }
}

fn relative(channel: u8, controller: u8, resolution: Resolution, action: Action) -> Binding {
    Binding {
        source: Source::ControlChange {
            channel,
            controller,
            resolution,
        },
        action,
    }
}

/// `None` for anything that reports a position, which has to be read through the
/// binding's range instead.
fn relative_delta(resolution: Resolution, value: u8) -> Option<i32> {
    let value = i32::from(value);
    match resolution {
        Resolution::CentreDelta => Some(value - RELATIVE_CENTRE),
        Resolution::SignedStep => Some(if value >= RELATIVE_CENTRE {
            value - SEVEN_BIT_WRAP
        } else {
            value
        }),
        Resolution::SevenBit | Resolution::FourteenBit => None,
    }
}

fn tempo_fader(channel: u8, controller: u8, deck: &str) -> Binding {
    Binding {
        source: Source::ControlChange {
            channel,
            controller,
            resolution: Resolution::FourteenBit,
        },
        action: Action::TempoFader {
            deck: deck.to_string(),
        },
    }
}

/// Read off a DDJ-FLX6 rather than taken from its documentation.
///
/// One loop covers mixer and deck controls alike because a mixer strip is wired
/// to its channel permanently while a deck half re-channels on deck select, and
/// both end up on the channel of the deck they address.
fn ddj_flx6() -> Profile {
    let mut bindings = Vec::new();
    for (channel, deck) in [(0, "A"), (1, "B"), (2, "C"), (3, "D")] {
        bindings.push(deck_param(channel, 7, deck, "eq", "high"));
        bindings.push(deck_param(channel, 11, deck, "eq", "mid"));
        bindings.push(deck_param(channel, 15, deck, "eq", "low"));
        bindings.push(deck_param(channel, 19, deck, "fader", "gain"));
        bindings.push(cue_toggle(channel, 84, deck));
        bindings.push(play_toggle(channel, 11, deck));
        bindings.push(transport_cue(channel, 12, deck));
        bindings.push(deck_note(
            channel,
            16,
            Action::LoopIn {
                deck: deck.to_string(),
            },
        ));
        bindings.push(deck_note(
            channel,
            17,
            Action::LoopOut {
                deck: deck.to_string(),
            },
        ));
        bindings.push(deck_note(
            channel,
            77,
            Action::LoopExitOrReloop {
                deck: deck.to_string(),
            },
        ));
        bindings.push(tempo_fader(channel, 0, deck));
        bindings.push(relative(
            channel,
            33,
            Resolution::CentreDelta,
            Action::Jog {
                deck: deck.to_string(),
            },
        ));
    }
    for (controller, deck) in [(23, "A"), (24, "B"), (25, "C"), (26, "D")] {
        bindings.push(deck_param(6, controller, deck, "filter", "value"));
    }
    bindings.push(xfader(6, 31));
    bindings.push(relative(6, 64, Resolution::SignedStep, Action::Browse));
    Profile::new(bindings).expect("the built-in DDJ-FLX6 profile")
}

#[derive(Default)]
struct ControlMemory {
    halves: HashMap<usize, (u8, u8)>,
}

impl ControlMemory {
    /// Acts on both halves instead of waiting for a pair to complete: a
    /// controller that sends only the half that changed would otherwise stall
    /// forever. The intermediate that a high half produces on its own is at
    /// most one low-half step away from the value the next message brings, so
    /// it is corrected before it can be heard.
    fn join(&mut self, binding: usize, half: Half, value: u8) -> u16 {
        let halves = self.halves.entry(binding).or_insert((0, 0));
        match half {
            Half::Msb => halves.0 = value,
            Half::Lsb => halves.1 = value,
        }
        (u16::from(halves.0) << 7) | u16::from(halves.1)
    }

    /// A half left over from before a reconnect would join with the first
    /// message after it and produce one value the knob was never at.
    fn clear(&mut self) {
        self.halves.clear();
    }
}

struct ControlChange {
    channel: u8,
    controller: u8,
    value: u8,
}

fn parse_control_change(data: &[u8]) -> Option<ControlChange> {
    let &[status, controller, value] = data else {
        return None;
    };
    if status & 0xF0 != CONTROL_CHANGE {
        return None;
    }
    Some(ControlChange {
        channel: status & 0x0F,
        controller,
        value: value & 0x7F,
    })
}

struct NoteOn {
    channel: u8,
    note: u8,
    velocity: u8,
}

fn parse_note_on(data: &[u8]) -> Option<NoteOn> {
    let &[status, note, velocity] = data else {
        return None;
    };
    if status & 0xF0 != NOTE_ON {
        return None;
    }
    Some(NoteOn {
        channel: status & 0x0F,
        note,
        velocity: velocity & 0x7F,
    })
}

/// Separated from applying it so the mapping path is testable without an
/// `AppState`, which cannot be built outside a running app.
#[derive(Debug, PartialEq)]
enum Move {
    Param {
        deck: String,
        slot: String,
        param: String,
        position: f64,
    },
    Cue {
        deck: String,
    },
    Play {
        deck: String,
    },
    CuePress {
        deck: String,
    },
    CueRelease {
        deck: String,
    },
    LoopIn {
        deck: String,
    },
    LoopOut {
        deck: String,
    },
    LoopExitOrReloop {
        deck: String,
    },
    Tempo {
        deck: String,
        position: f64,
    },
    Jog {
        deck: String,
        ticks: i32,
    },
    Browse {
        steps: i32,
    },
    Xfader {
        position: f64,
    },
}

fn resolve_move(profile: &Profile, memory: &mut ControlMemory, data: &[u8]) -> Option<Move> {
    if let Some(message) = parse_control_change(data) {
        let (index, binding, half) = profile.resolve(Key::ControlChange {
            channel: message.channel,
            controller: message.controller,
        })?;
        // Ahead of the position maths, and ahead of `join`, so a relative control
        // never leaves a half behind for a real pair to collide with.
        if let Some(delta) = relative_delta(binding.source.resolution(), message.value) {
            return match &binding.action {
                Action::Jog { deck } => (delta != 0).then(|| Move::Jog {
                    deck: deck.clone(),
                    ticks: delta,
                }),
                Action::Browse => (delta != 0).then_some(Move::Browse { steps: delta }),
                _ => None,
            };
        }
        let position = match binding.source.resolution() {
            Resolution::FourteenBit => {
                f64::from(memory.join(index, half, message.value)) / FOURTEEN_BIT_MAX
            }
            // A relative control returned above, so it never reaches this; the arm
            // is spelled out anyway so a new resolution has to be considered here.
            Resolution::SevenBit | Resolution::CentreDelta | Resolution::SignedStep => {
                f64::from(message.value) / SEVEN_BIT_MAX
            }
        };
        return match &binding.action {
            Action::DeckParam { deck, slot, param } => Some(Move::Param {
                deck: deck.clone(),
                slot: slot.clone(),
                param: param.clone(),
                position,
            }),
            Action::XfaderPosition => Some(Move::Xfader { position }),
            Action::TempoFader { deck } => Some(Move::Tempo {
                deck: deck.clone(),
                position,
            }),
            Action::CueToggle { .. }
            | Action::PlayToggle { .. }
            | Action::TransportCue { .. }
            | Action::LoopIn { .. }
            | Action::LoopOut { .. }
            | Action::LoopExitOrReloop { .. }
            | Action::Jog { .. }
            | Action::Browse => None,
        };
    }

    let message = parse_note_on(data)?;
    let (_, binding, _) = profile.resolve(Key::Note {
        channel: message.channel,
        note: message.note,
    })?;
    let pressed = message.velocity > 0;
    match &binding.action {
        // Toggles in the app, momentary buttons on the controller.
        Action::CueToggle { deck } => pressed.then(|| Move::Cue { deck: deck.clone() }),
        Action::PlayToggle { deck } => pressed.then(|| Move::Play { deck: deck.clone() }),
        Action::TransportCue { deck } => Some(if pressed {
            Move::CuePress { deck: deck.clone() }
        } else {
            Move::CueRelease { deck: deck.clone() }
        }),
        Action::LoopIn { deck } => pressed.then(|| Move::LoopIn { deck: deck.clone() }),
        Action::LoopOut { deck } => pressed.then(|| Move::LoopOut { deck: deck.clone() }),
        Action::LoopExitOrReloop { deck } => {
            pressed.then(|| Move::LoopExitOrReloop { deck: deck.clone() })
        }
        Action::DeckParam { .. }
        | Action::XfaderPosition
        | Action::TempoFader { .. }
        | Action::Jog { .. }
        | Action::Browse => None,
    }
}

pub(crate) fn apply(
    state: &crate::AppState,
    midi: &MidiState,
    app: &tauri::AppHandle,
    data: &[u8],
) {
    // The same gate the keyboard has in `useKeyboard.ts`. Outside performance the
    // session scheduler owns the strips, and it writes them through
    // `apply_deck_command`, which does not pass the `set_deck_param` funnel, so
    // nothing downstream would notice the two fighting.
    if state.app_mode() != crate::AppMode::Performance {
        return;
    }
    // Scoped so the memory lock is released before the engine locks are taken.
    let moved = {
        let mut memory = midi
            .memory
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        resolve_move(&midi.profile, &mut memory, data)
    };
    match moved {
        None => {}
        Some(Move::Cue { deck }) => {
            state
                .toggle_cue_active(crate::ParamOrigin::Midi, &deck)
                .ok();
        }
        Some(Move::Play { deck }) => {
            state.toggle_play(crate::ParamOrigin::Midi, &deck).ok();
        }
        Some(Move::CuePress { deck }) => {
            state.press_cue(crate::ParamOrigin::Midi, &deck).ok();
        }
        Some(Move::CueRelease { deck }) => {
            state.release_cue(crate::ParamOrigin::Midi, &deck).ok();
        }
        Some(Move::LoopIn { deck }) => {
            state.loop_in(crate::ParamOrigin::Midi, &deck).ok();
        }
        Some(Move::LoopOut { deck }) => {
            state.loop_out(crate::ParamOrigin::Midi, &deck).ok();
        }
        Some(Move::LoopExitOrReloop { deck }) => {
            state.exit_or_reloop(crate::ParamOrigin::Midi, &deck).ok();
        }
        Some(Move::Jog { deck, ticks }) => {
            state.jog(crate::ParamOrigin::Midi, &deck, ticks).ok();
        }
        // Selection is not engine state, so this is the one move Rust forwards
        // rather than acts on.
        Some(Move::Browse { steps }) => {
            app.emit("midi-browse", steps).ok();
        }
        Some(Move::Tempo { deck, position }) => {
            state
                .set_playback_rate_from_fader(crate::ParamOrigin::Midi, &deck, position)
                .ok();
        }
        Some(Move::Xfader { position }) => {
            let Some(descriptor) = state.audio.mixer().descriptor(
                session_core::ParamScope::Master,
                "xfader",
                "position",
            ) else {
                return;
            };
            let value = descriptor.from_unit_interval(position);
            state.set_xfader_position(crate::ParamOrigin::Midi, value as f32);
        }
        Some(Move::Param {
            deck,
            slot,
            param,
            position,
        }) => {
            let Some(descriptor) =
                state
                    .audio
                    .mixer()
                    .descriptor(session_core::ParamScope::Deck, &slot, &param)
            else {
                return;
            };
            let value = descriptor.from_unit_interval(position);
            state
                .set_deck_param(crate::ParamOrigin::Midi, &deck, &slot, &param, value as f32)
                .ok();
        }
    }
}

/// Lights a deck's cue button to match the app. Driven by every cue change
/// whatever caused it, so a mouse toggle lights the button too.
pub fn send_cue_led(state: &MidiState, deck: &str, active: bool) {
    let Some(Key::Note { channel, note }) = state.profile.cue_keys.get(deck).copied() else {
        return;
    };
    state.send(Request::Send(vec![
        NOTE_ON | channel,
        note,
        if active { 127 } else { 0 },
    ]));
}

/// Nothing else pushes state when a port opens, so a cue the app already has on
/// would leave its button dark until the next toggle.
fn resync_cue_leds(state: &crate::AppState, midi: &MidiState) {
    for deck in midi.profile.cue_keys.keys() {
        let Some(strip) = state.audio.strip(deck) else {
            continue;
        };
        let active = strip
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .cue_active;
        send_cue_led(midi, deck, active);
    }
}

pub fn start_monitor(app: tauri::AppHandle, state: &MidiState) {
    let monitor = Arc::clone(&state.monitor);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(MONITOR_FLUSH_MS));
        let batch = monitor.drain();
        if !batch.is_empty() {
            app.emit("midi-messages", batch).ok();
        }
    });
}

#[tauri::command]
pub fn list_midi_inputs() -> Result<Vec<String>, String> {
    let input = midir::MidiInput::new(CLIENT_NAME).map_err(|error| error.to_string())?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .collect())
}

#[tauri::command]
pub fn set_midi_input(
    state: tauri::State<'_, MidiState>,
    app_state: tauri::State<'_, crate::AppState>,
    port: Option<String>,
) -> Result<(), String> {
    state.clear_control_memory();
    let Some(port) = port else {
        state.send(Request::Disconnect);
        *state
            .connected
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        return Ok(());
    };
    let (reply, answer) = channel();
    state.send(Request::Connect(port.clone(), reply));
    answer
        .recv()
        .map_err(|_| "the MIDI thread stopped".to_string())??;
    *state
        .connected
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(port);
    // The reply arrives before the Connect arm opens the output, but requests are
    // served in order, so these are sent once that arm has finished.
    resync_cue_leds(&app_state, &state);
    Ok(())
}

#[tauri::command]
pub fn get_midi_input(state: tauri::State<'_, MidiState>) -> Option<String> {
    state
        .connected
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[tauri::command]
pub fn set_midi_monitor(state: tauri::State<'_, MidiState>, enabled: bool) {
    state.monitor.enabled.store(enabled, Ordering::Relaxed);
    if !enabled {
        state.monitor.drain();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(byte: u8) -> MidiMessage {
        MidiMessage {
            port: "test".to_string(),
            timestamp_us: byte as u64,
            data: vec![0xB0, 0x01, byte],
        }
    }

    fn control_change(channel: u8, controller: u8, value: u8) -> Vec<u8> {
        vec![CONTROL_CHANGE | channel, controller, value]
    }

    #[test]
    fn a_control_change_decodes_to_its_channel_controller_and_value() {
        let message = parse_control_change(&control_change(2, 20, 64)).expect("a control change");
        assert_eq!(
            (message.channel, message.controller, message.value),
            (2, 20, 64)
        );
    }

    #[test]
    fn anything_that_is_not_a_three_byte_control_change_is_ignored() {
        // Note on, pitch bend, a running-status runt, and an empty buffer.
        assert!(parse_control_change(&[0x90, 60, 100]).is_none());
        assert!(parse_control_change(&[0xE0, 0, 64]).is_none());
        assert!(parse_control_change(&[0xB0, 20]).is_none());
        assert!(parse_control_change(&[]).is_none());
    }

    fn note_on(channel: u8, note: u8, velocity: u8) -> Vec<u8> {
        vec![NOTE_ON | channel, note, velocity]
    }

    #[test]
    fn both_halves_of_a_bound_control_resolve_and_an_unbound_one_does_not() {
        let profile = ddj_flx6();
        let (_, binding, half) = profile
            .resolve(Key::ControlChange {
                channel: 0,
                controller: 15,
            })
            .expect("cc 15 is bound");
        let Action::DeckParam { deck, slot, param } = &binding.action else {
            panic!("cc 15 is a deck param");
        };
        assert_eq!(
            (deck.as_str(), slot.as_str(), param.as_str()),
            ("A", "eq", "low")
        );
        assert_eq!(half, Half::Msb);

        let (_, _, half) = profile
            .resolve(Key::ControlChange {
                channel: 0,
                controller: 47,
            })
            .expect("cc 47 is the low half of cc 15");
        assert_eq!(half, Half::Lsb);

        assert!(profile
            .resolve(Key::ControlChange {
                channel: 0,
                controller: 99
            })
            .is_none());
        // The low half exists only on the binding's own channel.
        assert!(profile
            .resolve(Key::ControlChange {
                channel: 5,
                controller: 47
            })
            .is_none());
    }

    // Read off the hardware: the crossfader is cc 31 on the filters' channel,
    // with its low half at cc 63, and it sweeps the full -1..+1 of the master
    // descriptor rather than a strip's range.
    #[test]
    fn the_crossfader_sweeps_end_to_end() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();

        let left = resolve_move(&profile, &mut halves, &control_change(6, 63, 0));
        assert_eq!(left, Some(Move::Xfader { position: 0.0 }));

        resolve_move(&profile, &mut halves, &control_change(6, 31, 127));
        let right = resolve_move(&profile, &mut halves, &control_change(6, 63, 127));
        assert_eq!(right, Some(Move::Xfader { position: 1.0 }));
    }

    // The exact interleave from the hardware, high half then low half, which is
    // what the controller actually sends.
    #[test]
    fn the_crossfaders_two_halves_join() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();

        resolve_move(&profile, &mut halves, &control_change(6, 31, 122));
        let moved = resolve_move(&profile, &mut halves, &control_change(6, 63, 127));
        let Some(Move::Xfader { position }) = moved else {
            panic!("cc 31/63 is the crossfader");
        };
        assert!((position - f64::from((122 << 7) | 127) / 16383.0).abs() < 1e-12);
    }

    // Its low half is cc 63, which must not land on the filters' 55-58 block.
    #[test]
    fn the_crossfader_does_not_collide_with_the_filters() {
        let profile = ddj_flx6();
        for controller in [55, 56, 57, 58] {
            let (_, binding, _) = profile
                .resolve(Key::ControlChange {
                    channel: 6,
                    controller,
                })
                .expect("a filter low half");
            assert!(matches!(binding.action, Action::DeckParam { .. }));
        }
    }

    #[test]
    fn a_cue_note_resolves_to_its_deck() {
        let profile = ddj_flx6();
        let (_, binding, _) = profile
            .resolve(Key::Note {
                channel: 2,
                note: 84,
            })
            .expect("note 84 on channel 2 is bound");
        let Action::CueToggle { deck } = &binding.action else {
            panic!("note 84 is a cue toggle");
        };
        assert_eq!(deck, "C");
    }

    // Each deck owns a channel and each control a CC, so a crossed pair would
    // silently move the wrong deck's knob.
    #[test]
    fn a_profile_whose_sources_collide_is_refused() {
        let collision = Profile::new(vec![
            deck_param(0, 7, "A", "eq", "high"),
            deck_param(0, 7, "B", "eq", "high"),
        ]);
        assert!(collision.is_err());
    }

    // The collision a table of declared addresses cannot see: cc 7's implied low
    // half is cc 39, so binding cc 39 as well silently steals it.
    #[test]
    fn an_implied_low_half_colliding_with_another_high_half_is_refused() {
        let collision = Profile::new(vec![
            deck_param(0, 7, "A", "eq", "high"),
            deck_param(0, 39, "A", "eq", "mid"),
        ]);
        assert!(collision.is_err());
    }

    #[test]
    fn a_control_change_too_high_to_carry_a_low_half_is_refused() {
        assert!(Profile::new(vec![deck_param(0, 96, "A", "eq", "high")]).is_err());
        assert!(Profile::new(vec![deck_param(0, 95, "A", "eq", "high")]).is_ok());
    }

    // The exact stream from the screenshot, high half first, ending at the 8192
    // centre of the 14-bit range.
    #[test]
    fn the_two_halves_of_a_high_resolution_control_join() {
        let mut halves = ControlMemory::default();
        assert_eq!(halves.join(0, Half::Msb, 61), 61 << 7);
        assert_eq!(halves.join(0, Half::Lsb, 75), (61 << 7) | 75);
        assert_eq!(halves.join(0, Half::Msb, 62), (62 << 7) | 75);
        assert_eq!(halves.join(0, Half::Lsb, 50), (62 << 7) | 50);
        assert_eq!(halves.join(0, Half::Msb, 64), (64 << 7) | 50);
        assert_eq!(halves.join(0, Half::Lsb, 0), 8192);
    }

    // Acting on a lone high half is what stops a controller that sends only the
    // half that moved from stalling, and it has to land within one low-half step.
    #[test]
    fn a_high_half_on_its_own_lands_within_one_low_half_step() {
        let mut halves = ControlMemory::default();
        halves.join(0, Half::Msb, 61);
        halves.join(0, Half::Lsb, 127);
        let intermediate = halves.join(0, Half::Msb, 62);
        let settled = halves.join(0, Half::Lsb, 0);
        assert!(u32::from(intermediate).abs_diff(u32::from(settled)) <= 127);
    }

    #[test]
    fn two_high_resolution_controls_do_not_share_halves() {
        let mut halves = ControlMemory::default();
        halves.join(0, Half::Msb, 64);
        assert_eq!(halves.join(1, Half::Lsb, 3), 3);
    }

    // A half surviving a reconnect would join with the first message after it
    // and place the knob somewhere it has never been.
    #[test]
    fn clearing_forgets_a_half() {
        let mut halves = ControlMemory::default();
        halves.join(0, Half::Msb, 127);
        halves.clear();
        assert_eq!(halves.join(0, Half::Lsb, 3), 3);
    }

    #[test]
    fn a_high_resolution_control_spans_the_whole_param_range() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();

        let low = resolve_move(&profile, &mut halves, &control_change(0, 47, 0));
        assert!(matches!(low, Some(Move::Param { position, .. }) if position == 0.0));
        resolve_move(&profile, &mut halves, &control_change(0, 15, 127));
        let high = resolve_move(&profile, &mut halves, &control_change(0, 47, 127));
        assert!(matches!(high, Some(Move::Param { position, .. }) if position == 1.0));
    }

    #[test]
    fn a_cue_press_toggles_and_its_release_does_not() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 84, 127)),
            Some(Move::Cue {
                deck: "B".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 84, 0)),
            None
        );
    }

    #[test]
    fn a_play_press_toggles_and_its_release_does_not() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 11, 127)),
            Some(Move::Play {
                deck: "A".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 11, 0)),
            None
        );
    }

    // Dropping the release would leave the deck previewing from the cue point
    // forever, with the button already back up.
    #[test]
    fn a_transport_cue_press_and_release_are_both_moves() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 12, 127)),
            Some(Move::CuePress {
                deck: "A".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 12, 0)),
            Some(Move::CueRelease {
                deck: "A".to_string()
            })
        );
    }

    // The transport cue and the headphone cue are different buttons on different
    // halves of the surface, and only the latter has an LED the app drives.
    #[test]
    fn transport_cue_is_not_the_headphone_cue() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(2, 84, 127)),
            Some(Move::Cue {
                deck: "C".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(2, 12, 127)),
            Some(Move::CuePress {
                deck: "C".to_string()
            })
        );
        assert_eq!(
            profile.cue_keys.get("C"),
            Some(&Key::Note {
                channel: 2,
                note: 84
            })
        );
    }

    // Deck select re-channels a deck half, so a transport press on the C layer
    // has to reach deck C without the profile knowing layers exist.
    #[test]
    fn transport_reaches_all_four_decks_by_channel() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        for (channel, deck) in [(0, "A"), (1, "B"), (2, "C"), (3, "D")] {
            assert_eq!(
                resolve_move(&profile, &mut halves, &note_on(channel, 11, 127)),
                Some(Move::Play {
                    deck: deck.to_string()
                })
            );
        }
    }

    #[test]
    fn the_three_loop_buttons_resolve_to_their_own_moves() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 16, 127)),
            Some(Move::LoopIn {
                deck: "B".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 17, 127)),
            Some(Move::LoopOut {
                deck: "B".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 77, 127)),
            Some(Move::LoopExitOrReloop {
                deck: "B".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 16, 0)),
            None
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 17, 0)),
            None
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(1, 77, 0)),
            None
        );
    }

    // Read off the hardware: the tempo fader is cc 0 with its low half at cc 32,
    // and the low half arrives first.
    #[test]
    fn the_tempo_fader_is_a_high_resolution_pair() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();

        resolve_move(&profile, &mut halves, &control_change(0, 32, 0));
        let bottom = resolve_move(&profile, &mut halves, &control_change(0, 0, 0));
        assert_eq!(
            bottom,
            Some(Move::Tempo {
                deck: "A".to_string(),
                position: 0.0
            })
        );

        resolve_move(&profile, &mut halves, &control_change(0, 0, 127));
        let top = resolve_move(&profile, &mut halves, &control_change(0, 32, 127));
        assert_eq!(
            top,
            Some(Move::Tempo {
                deck: "A".to_string(),
                position: 1.0
            })
        );
    }

    // Read off the hardware: a steady turn holds one value just off centre, so
    // the wheel reports speed, not the angle it is at.
    #[test]
    fn the_jog_reports_deviation_from_its_centre() {
        let profile = ddj_flx6();
        let mut memory = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut memory, &control_change(0, 33, 70)),
            Some(Move::Jog {
                deck: "A".to_string(),
                ticks: 6
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut memory, &control_change(0, 33, 63)),
            Some(Move::Jog {
                deck: "A".to_string(),
                ticks: -1
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut memory, &control_change(0, 33, 64)),
            None
        );
    }

    // The other relative encoding on this surface: one detent either way, which
    // reads as 1 and 127 rather than as an angle.
    #[test]
    fn the_browse_encoder_reports_a_signed_step() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(6, 64, 1)),
            Some(Move::Browse { steps: 1 })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(6, 64, 127)),
            Some(Move::Browse { steps: -1 })
        );
    }

    // A relative control declares no low half, so binding one must not reserve
    // the cc 32 above it and steal a real address.
    #[test]
    fn a_relative_control_claims_no_low_half() {
        let profile = ddj_flx6();
        assert!(profile
            .resolve(Key::ControlChange {
                channel: 0,
                controller: 65
            })
            .is_none());
        assert!(profile
            .resolve(Key::ControlChange {
                channel: 6,
                controller: 96
            })
            .is_none());
    }

    #[test]
    fn an_unmapped_message_is_no_move() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(9, 99, 64)),
            None
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(9, 60, 127)),
            None
        );
        // Clock, a running-status runt, and an empty buffer.
        assert_eq!(resolve_move(&profile, &mut halves, &[0xF8]), None);
        assert_eq!(resolve_move(&profile, &mut halves, &[0xB0, 20]), None);
        assert_eq!(resolve_move(&profile, &mut halves, &[]), None);
    }

    // A binding naming an address the mixer does not have would silently do
    // nothing at runtime, which is indistinguishable from a broken controller.
    #[test]
    fn every_binding_addresses_a_real_param() {
        for binding in &ddj_flx6().bindings {
            let Action::DeckParam { slot, param, .. } = &binding.action else {
                continue;
            };
            assert!(
                session_core::CLASSIC_3BAND
                    .descriptor(session_core::ParamScope::Deck, slot, param)
                    .is_some(),
                "{slot}/{param}"
            );
        }
    }

    // The LED path is a reverse lookup, so a cue binding the profile can dispatch
    // but not light would leave the button dark whatever the app did.
    #[test]
    fn every_cue_binding_has_a_key_to_light() {
        let profile = ddj_flx6();
        for deck in ["A", "B", "C", "D"] {
            assert!(profile.cue_keys.contains_key(deck), "{deck}");
        }
    }

    #[test]
    fn a_disabled_monitor_buffers_nothing() {
        let monitor = Monitor::new();
        monitor.push(message(1));
        assert!(monitor.drain().is_empty());
    }

    #[test]
    fn a_controller_outrunning_the_flush_drops_the_oldest_rather_than_growing() {
        let monitor = Monitor::new();
        monitor.enabled.store(true, Ordering::Relaxed);
        for index in 0..MONITOR_CAPACITY + 10 {
            monitor.push(message((index % 128) as u8));
        }

        let drained = monitor.drain();
        assert_eq!(drained.len(), MONITOR_CAPACITY);
        assert_eq!(drained[0].timestamp_us, 10);
        assert!(monitor.drain().is_empty());
    }
}
