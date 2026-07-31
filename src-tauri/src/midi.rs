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
    /// The whole set of ports that should be open. Anything already open and still named is
    /// left alone, so a rescan does not interrupt a device the user is playing.
    Connect(Vec<String>, Sender<()>),
    Send(String, Vec<u8>),
}

type Dispatch = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;
type DispatchSlot = Arc<Mutex<Option<Dispatch>>>;

/// One connected port, with its own 14-bit memory. The memory is per device because
/// halves from two controllers must never join.
struct Device {
    mapping: Option<usize>,
    profile: Option<Profile>,
    memory: ControlMemory,
    deck: Option<String>,
}

pub struct MidiState {
    requests: Mutex<Sender<Request>>,
    monitor: Arc<Monitor>,
    dispatch: DispatchSlot,
    mappings: Vec<Mapping>,
    devices: Mutex<HashMap<String, Device>>,
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
            mappings: built_in_mappings(),
            devices: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn clear_control_memory(&self) {
        for device in self
            .devices
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values_mut()
        {
            device.memory.clear();
        }
    }

    fn mapping_for(&self, port: &str) -> Option<usize> {
        self.mappings
            .iter()
            .position(|mapping| mapping.claims(port))
    }

    /// A port that is still present is left untouched, so a rescan never disturbs
    /// the deck assignment of a device someone is playing.
    fn sync_devices(&self, ports: &[String]) {
        let mut devices = self
            .devices
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        devices.retain(|port, _| ports.contains(port));
        for port in ports {
            if devices.contains_key(port) {
                continue;
            }
            let mapping = self.mapping_for(port);
            let profile = mapping.and_then(|index| {
                let mapping = &self.mappings[index];
                if mapping.needs_deck() {
                    return None;
                }
                mapping.profile(None).ok()
            });
            devices.insert(
                port.clone(),
                Device {
                    mapping,
                    profile,
                    memory: ControlMemory::default(),
                    deck: None,
                },
            );
        }
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
    let mut inputs: HashMap<String, midir::MidiInputConnection<()>> = HashMap::new();
    let mut outputs: HashMap<String, midir::MidiOutputConnection> = HashMap::new();
    while let Ok(request) = requests.recv() {
        match request {
            Request::Send(port_name, bytes) => {
                if let Some(port) = outputs.get_mut(&port_name) {
                    let _ = port.send(&bytes);
                }
            }
            Request::Connect(port_names, reply) => {
                // Dropping the binding closes the port, so a device that has gone
                // away is closed by being forgotten here.
                inputs.retain(|name, _| port_names.contains(name));
                outputs.retain(|name, _| port_names.contains(name));
                // A controller that allows one client at a time would refuse a second
                // connection, so an already open port is left alone.
                for port_name in ports_to_open(&port_names, |name| inputs.contains_key(name)) {
                    let opened = connect(&port_name, Arc::clone(&monitor), Arc::clone(&dispatch));
                    if let Ok(open) = opened {
                        inputs.insert(port_name, open);
                    }
                }
                // Feedback is a bonus, not a requirement, and it is retried independently
                // of the input so a port that was busy once does not stay dark all session.
                for port_name in ports_to_open(&port_names, |name| outputs.contains_key(name)) {
                    if let Some(output) = connect_output(&port_name) {
                        outputs.insert(port_name, output);
                    }
                }
                let _ = reply.send(());
            }
        }
    }
    drop(inputs);
    drop(outputs);
}

fn ports_to_open(wanted: &[String], is_open: impl Fn(&str) -> bool) -> Vec<String> {
    wanted
        .iter()
        .filter(|name| !is_open(name))
        .cloned()
        .collect()
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
                    handler(&source, data);
                }
            },
            (),
        )
        .map_err(|error| error.to_string())
}

/// The only path from the MIDI thread into the app, so mapped input cannot reach device
/// or buffer configuration, which rebuild the streams and stay on the main thread.
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

/// What a lit button reports. A binding is an input address; this says which of
/// them the app also writes back to, so a control with no entry stays dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Feedback {
    Cue,
    Quantize,
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
    // Held, not latched: the surface sends both edges and the deck it names is
    // the one whose wheel it modifies.
    Shift {
        deck: String,
    },
    QuantizeToggle {
        deck: String,
    },
    Eject {
        deck: String,
    },
    // `steps` is how far one press moves the cursor, for a surface that browses with buttons.
    // An encoder carries its own delta and leaves this unset.
    Browse {
        steps: Option<i32>,
    },
    Load {
        deck: String,
    },
    Enter,
    Back,
    ToggleView,
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
    led_keys: HashMap<(Feedback, String), Key>,
}

impl Profile {
    /// Refuses a colliding profile rather than letting the last binding win, because the
    /// symptom is the wrong deck's control moving, which reads as broken hardware.
    fn new(bindings: Vec<Binding>) -> Result<Self, String> {
        let mut by_key = HashMap::new();
        let mut led_keys = HashMap::new();
        for (index, binding) in bindings.iter().enumerate() {
            let keys = binding.source.keys()?;
            let lit = match &binding.action {
                Action::CueToggle { deck } => Some((Feedback::Cue, deck)),
                Action::QuantizeToggle { deck } => Some((Feedback::Quantize, deck)),
                _ => None,
            };
            if let Some((feedback, deck)) = lit {
                if let Some((key, _)) = keys.first() {
                    led_keys.insert((feedback, deck.clone()), *key);
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
            led_keys,
        })
    }

    fn resolve(&self, key: Key) -> Option<(usize, &Binding, Half)> {
        let &(index, half) = self.by_key.get(&key)?;
        Some((index, &self.bindings[index], half))
    }
}

/// A control cannot send its own midpoint (63.5 of 7 bits), so a plain `value / max` puts
/// a detent past half. On a 7-bit tempo fader at 10% that showed 141 bpm as 141.11.
fn unit_interval(value: f64, max: f64) -> f64 {
    let centre = (max + 1.0) / 2.0;
    if value <= centre {
        value / (centre * 2.0)
    } else {
        0.5 + (value - centre) / (max - centre) / 2.0
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

/// Bumped only when the vocabulary changes. A file declaring a newer version is
/// refused rather than half-read.
const MAPPING_VERSION: u32 = 1;

const MAPPING_FILES: [&str; 2] = [
    include_str!("../mappings/ddj-flx6.json"),
    include_str!("../mappings/xdj-1000mk2.json"),
];

#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum DeckScope {
    Fixed,
    Assigned,
}

#[derive(serde::Deserialize, Clone, Copy)]
enum ResolutionSpec {
    #[serde(rename = "7bit")]
    SevenBit,
    #[serde(rename = "14bit")]
    FourteenBit,
    #[serde(rename = "centre_delta")]
    CentreDelta,
    #[serde(rename = "signed_step")]
    SignedStep,
}

impl From<ResolutionSpec> for Resolution {
    fn from(spec: ResolutionSpec) -> Self {
        match spec {
            ResolutionSpec::SevenBit => Resolution::SevenBit,
            ResolutionSpec::FourteenBit => Resolution::FourteenBit,
            ResolutionSpec::CentreDelta => Resolution::CentreDelta,
            ResolutionSpec::SignedStep => Resolution::SignedStep,
        }
    }
}

#[derive(serde::Deserialize)]
struct BindingSpec {
    channel: u8,
    action: String,
    #[serde(default)]
    note: Option<u8>,
    #[serde(default)]
    cc: Option<u8>,
    #[serde(default)]
    resolution: Option<ResolutionSpec>,
    #[serde(default)]
    deck: Option<String>,
    #[serde(default)]
    slot: Option<String>,
    #[serde(default)]
    param: Option<String>,
    #[serde(default)]
    steps: Option<i32>,
}

struct Mapping {
    name: String,
    matches: Vec<String>,
    decks: DeckScope,
    bindings: Vec<BindingSpec>,
}

#[derive(serde::Deserialize)]
struct MappingFile {
    version: u32,
    name: String,
    #[serde(rename = "match")]
    matches: Vec<String>,
    decks: DeckScope,
    bindings: Vec<BindingSpec>,
}

impl BindingSpec {
    /// `assigned` is the deck the user gave the device, and is what an `assigned` mapping's
    /// bindings are built against. A `fixed` mapping names its own decks and ignores it.
    fn deck(&self, scope: DeckScope, assigned: Option<&str>) -> Result<String, String> {
        match scope {
            DeckScope::Fixed => self
                .deck
                .clone()
                .ok_or_else(|| format!("'{}' needs a deck", self.action)),
            DeckScope::Assigned => assigned
                .map(str::to_string)
                .ok_or_else(|| "the device has no deck".to_string()),
        }
    }

    fn source(&self) -> Result<Source, String> {
        // The file counts channels the way the hardware's documentation and the
        // console monitor do, from one.
        let channel = self
            .channel
            .checked_sub(1)
            .ok_or_else(|| "channel is counted from 1".to_string())?;
        match (self.note, self.cc) {
            (Some(note), None) => Ok(Source::Note { channel, note }),
            (None, Some(controller)) => Ok(Source::ControlChange {
                channel,
                controller,
                resolution: self
                    .resolution
                    .map_or(Resolution::SevenBit, Resolution::from),
            }),
            _ => Err("a binding needs exactly one of 'note' or 'cc'".to_string()),
        }
    }

    fn build(&self, scope: DeckScope, assigned: Option<&str>) -> Result<Binding, String> {
        let action = match self.action.as_str() {
            "deck_param" => Action::DeckParam {
                deck: self.deck(scope, assigned)?,
                slot: self
                    .slot
                    .clone()
                    .ok_or_else(|| "'deck_param' needs a slot".to_string())?,
                param: self
                    .param
                    .clone()
                    .ok_or_else(|| "'deck_param' needs a param".to_string())?,
            },
            "cue_toggle" => Action::CueToggle {
                deck: self.deck(scope, assigned)?,
            },
            "play_toggle" => Action::PlayToggle {
                deck: self.deck(scope, assigned)?,
            },
            "transport_cue" => Action::TransportCue {
                deck: self.deck(scope, assigned)?,
            },
            "loop_in" => Action::LoopIn {
                deck: self.deck(scope, assigned)?,
            },
            "loop_out" => Action::LoopOut {
                deck: self.deck(scope, assigned)?,
            },
            "loop_exit_or_reloop" => Action::LoopExitOrReloop {
                deck: self.deck(scope, assigned)?,
            },
            "tempo_fader" => Action::TempoFader {
                deck: self.deck(scope, assigned)?,
            },
            "jog" => Action::Jog {
                deck: self.deck(scope, assigned)?,
            },
            "shift" => Action::Shift {
                deck: self.deck(scope, assigned)?,
            },
            "quantize_toggle" => Action::QuantizeToggle {
                deck: self.deck(scope, assigned)?,
            },
            "eject" => Action::Eject {
                deck: self.deck(scope, assigned)?,
            },
            "load" => Action::Load {
                deck: self.deck(scope, assigned)?,
            },
            "browse" => Action::Browse { steps: self.steps },
            "enter" => Action::Enter,
            "back" => Action::Back,
            "toggle_view" => Action::ToggleView,
            "xfader_position" => Action::XfaderPosition,
            other => return Err(format!("unknown action '{other}'")),
        };
        Ok(Binding {
            source: self.source()?,
            action,
        })
    }
}

impl Mapping {
    fn name(&self) -> &str {
        &self.name
    }

    fn claims(&self, port: &str) -> bool {
        self.matches.iter().any(|needle| port.contains(needle))
    }

    fn needs_deck(&self) -> bool {
        self.decks == DeckScope::Assigned
    }

    fn profile(&self, assigned: Option<&str>) -> Result<Profile, String> {
        let bindings = self
            .bindings
            .iter()
            .map(|spec| spec.build(self.decks, assigned))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{}: {error}", self.name))?;
        Profile::new(bindings).map_err(|error| format!("{}: {error}", self.name))
    }
}

fn parse_mapping(source: &str) -> Result<Mapping, String> {
    let file: MappingFile = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if file.version > MAPPING_VERSION {
        return Err(format!(
            "{} declares version {}, newer than {MAPPING_VERSION}",
            file.name, file.version
        ));
    }
    Ok(Mapping {
        name: file.name,
        matches: file.matches,
        decks: file.decks,
        bindings: file.bindings,
    })
}

fn built_in_mappings() -> Vec<Mapping> {
    MAPPING_FILES
        .iter()
        .map(|source| parse_mapping(source).expect("a built-in mapping file"))
        .collect()
}

#[derive(Default)]
struct ControlMemory {
    halves: HashMap<usize, (u8, u8)>,
}

impl ControlMemory {
    /// Acts on both halves rather than waiting for a pair, or a controller sending only
    /// the changed half stalls. A lone high half lands within one low-half step.
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
    Shift {
        deck: String,
        held: bool,
    },
    QuantizeToggle {
        deck: String,
    },
    Eject {
        deck: String,
    },
    Browse {
        steps: i32,
    },
    Load {
        deck: String,
    },
    Enter,
    Back,
    ToggleView,
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
                Action::Browse { .. } => (delta != 0).then_some(Move::Browse { steps: delta }),
                _ => None,
            };
        }
        let position = match binding.source.resolution() {
            Resolution::FourteenBit => unit_interval(
                f64::from(memory.join(index, half, message.value)),
                FOURTEEN_BIT_MAX,
            ),
            // A relative control returned above, so it never reaches this; the arm
            // is spelled out anyway so a new resolution has to be considered here.
            Resolution::SevenBit | Resolution::CentreDelta | Resolution::SignedStep => {
                unit_interval(f64::from(message.value), SEVEN_BIT_MAX)
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
            | Action::Shift { .. }
            | Action::QuantizeToggle { .. }
            | Action::Eject { .. }
            | Action::Browse { .. }
            | Action::Load { .. }
            | Action::Enter
            | Action::Back
            | Action::ToggleView => None,
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
        Action::Shift { deck } => Some(Move::Shift {
            deck: deck.clone(),
            held: pressed,
        }),
        // Momentary on the surface, latching in the app, so only the press turns
        // it over; the release would turn it straight back.
        Action::QuantizeToggle { deck } => {
            pressed.then(|| Move::QuantizeToggle { deck: deck.clone() })
        }
        Action::Eject { deck } => pressed.then(|| Move::Eject { deck: deck.clone() }),
        Action::Load { deck } => pressed.then(|| Move::Load { deck: deck.clone() }),
        Action::Enter => pressed.then_some(Move::Enter),
        Action::Back => pressed.then_some(Move::Back),
        Action::ToggleView => pressed.then_some(Move::ToggleView),
        Action::Browse { steps } => steps
            .filter(|_| pressed)
            .map(|steps| Move::Browse { steps }),
        Action::DeckParam { .. }
        | Action::XfaderPosition
        | Action::TempoFader { .. }
        | Action::Jog { .. } => None,
    }
}

pub(crate) fn apply(
    state: &crate::AppState,
    midi: &MidiState,
    app: &tauri::AppHandle,
    port: &str,
    data: &[u8],
) {
    // The same gate the keyboard has in `useKeyboard.ts`. Outside performance the session
    // scheduler owns the strips and writes them past the `set_deck_param` funnel.
    if state.app_mode() != crate::AppMode::Performance {
        return;
    }
    // Scoped so the device lock is released before the engine locks are taken.
    let moved = {
        let mut devices = midi
            .devices
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(device) = devices.get_mut(port) else {
            return;
        };
        let Device {
            profile, memory, ..
        } = device;
        let Some(profile) = profile.as_ref() else {
            return;
        };
        resolve_move(profile, memory, data)
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
        Some(Move::Shift { deck, held }) => {
            state.set_jog_shift(&deck, held).ok();
        }
        // Forwarded rather than acted on: the frontend owns the flag and writes
        // it back through `set_quantize`, which is also what lights the button.
        Some(Move::QuantizeToggle { deck }) => {
            app.emit("midi-quantize", deck).ok();
        }
        // Selection lives in the frontend, so Rust forwards these rather than acting on them.
        // Which track a load button loads is only knowable from the cursor.
        Some(Move::Browse { steps }) => {
            app.emit("midi-browse", steps).ok();
        }
        Some(Move::Enter) => {
            app.emit("midi-enter", ()).ok();
        }
        Some(Move::Back) => {
            app.emit("midi-back", ()).ok();
        }
        Some(Move::ToggleView) => {
            app.emit("midi-toggle-view", ()).ok();
        }
        Some(Move::Load { deck }) => {
            app.emit("midi-load", deck).ok();
        }
        Some(Move::Eject { deck }) => {
            app.emit("midi-eject", deck).ok();
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

/// Driven by every change whatever caused it, so a mouse toggle lights the
/// button too.
pub fn send_led(state: &MidiState, kind: Feedback, deck: &str, active: bool) {
    let writes: Vec<(String, u8, u8)> = {
        let devices = state
            .devices
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        devices
            .iter()
            .filter_map(|(port, device)| {
                match device
                    .profile
                    .as_ref()?
                    .led_keys
                    .get(&(kind, deck.to_string()))?
                {
                    Key::Note { channel, note } => Some((port.clone(), *channel, *note)),
                    Key::ControlChange { .. } => None,
                }
            })
            .collect()
    };
    for (port, channel, note) in writes {
        state.send(Request::Send(
            port,
            vec![NOTE_ON | channel, note, if active { 127 } else { 0 }],
        ));
    }
}

/// Nothing else pushes state when a port opens, so a button the app already has
/// on would stay dark until the next toggle.
fn resync_leds(state: &crate::AppState, midi: &MidiState) {
    let lit: Vec<(Feedback, String)> = {
        let devices = midi
            .devices
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut lit: Vec<(Feedback, String)> = devices
            .values()
            .filter_map(|device| device.profile.as_ref())
            .flat_map(|profile| profile.led_keys.keys().cloned())
            .collect();
        lit.sort();
        lit.dedup();
        lit
    };
    for (kind, deck) in lit {
        let on = match kind {
            Feedback::Cue => state.audio.strip(&deck).map(|strip| {
                strip
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .cue_active
            }),
            Feedback::Quantize => state.deck(&deck).ok().map(|deck_state| {
                deck_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .quantize
            }),
        };
        if let Some(on) = on {
            send_led(midi, kind, &deck, on);
        }
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiDevice {
    port: String,
    mapping: Option<String>,
    assignable: bool,
    deck: Option<String>,
}

fn port_names() -> Result<Vec<String>, String> {
    let input = midir::MidiInput::new(CLIENT_NAME).map_err(|error| error.to_string())?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .collect())
}

fn device_list(state: &MidiState) -> Vec<MidiDevice> {
    let devices = state
        .devices
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut list: Vec<MidiDevice> = devices
        .iter()
        .map(|(port, device)| MidiDevice {
            port: port.clone(),
            mapping: device
                .mapping
                .map(|index| state.mappings[index].name().to_string()),
            assignable: device
                .mapping
                .is_some_and(|index| state.mappings[index].needs_deck()),
            deck: device.deck.clone(),
        })
        .collect();
    list.sort_by(|a, b| a.port.cmp(&b.port));
    list
}

/// Every connected port comes online at once, each through its own mapping.
#[tauri::command]
pub fn list_midi_devices(
    state: tauri::State<'_, MidiState>,
    app_state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<MidiDevice>, String> {
    let ports = port_names()?;
    state.sync_devices(&ports);
    let (reply, answer) = channel();
    state.send(Request::Connect(ports, reply));
    answer
        .recv()
        .map_err(|_| "the MIDI thread stopped".to_string())?;
    // Requests are served in order, so the outputs these write to are open.
    resync_leds(&app_state, &state);
    Ok(device_list(&state))
}

/// A device whose mapping names no decks of its own plays one deck, and this is
/// the only thing that says which.
#[tauri::command]
pub fn set_midi_device_deck(
    state: tauri::State<'_, MidiState>,
    app_state: tauri::State<'_, crate::AppState>,
    port: String,
    deck: Option<String>,
) -> Result<(), String> {
    {
        let mut devices = state
            .devices
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(device) = devices.get_mut(&port) else {
            return Err(format!("no MIDI device on '{port}'"));
        };
        let Some(index) = device.mapping else {
            return Err(format!("'{port}' has no mapping"));
        };
        device.deck = deck.clone();
        device.memory.clear();
        device.profile = match deck {
            Some(deck) => Some(state.mappings[index].profile(Some(&deck))?),
            None => None,
        };
    }
    // Rebuilding the profile discards the binding that would have delivered the release of
    // anything still held, and a reassignment would route it to the new deck anyway.
    app_state.audio.release_held_controls();
    // The profile only exists once a deck is chosen, so the resync in
    // `list_midi_devices` ran against a device that could not be lit yet.
    resync_leds(&app_state, &state);
    Ok(())
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

    /// Every control test below runs against the profile built from the shipped
    /// mapping file, so a format that cannot reproduce the mapping fails here.
    fn ddj_flx6() -> Profile {
        mapping_named("DDJ-FLX6")
            .profile(None)
            .expect("the DDJ-FLX6 mapping")
    }

    fn mapping_named(name: &str) -> Mapping {
        built_in_mappings()
            .into_iter()
            .find(|mapping| mapping.name() == name)
            .unwrap_or_else(|| panic!("no built-in mapping named {name}"))
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

    // Read off the hardware: the crossfader is cc 31 on the filters' channel with its low
    // half at cc 63, and it sweeps the master descriptor's full -1..+1.
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
        // Joined into one 14-bit value, then read through the centre: above the
        // detent the upper half of the throw spans 8192..16383.
        let joined = f64::from((122 << 7) | 127);
        let expected = 0.5 + (joined - 8192.0) / (16383.0 - 8192.0) / 2.0;
        assert!((position - expected).abs() < 1e-12);
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
            profile.led_keys.get(&(Feedback::Cue, "C".to_string())),
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

    // Read off the hardware: the tempo fader is cc 0 with its low half at cc 32. Both orders
    // are exercised below because the join deliberately does not care which arrives first.
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

        // Parked in the detent the surface sends cc 0 = 64 and cc 32 = 0, which joins to 8192 and
        // has to read as exactly half or the deck plays at a rate the fader never asked for.
        resolve_move(&profile, &mut halves, &control_change(0, 0, 64));
        let centre = resolve_move(&profile, &mut halves, &control_change(0, 32, 0));
        assert_eq!(
            centre,
            Some(Move::Tempo {
                deck: "A".to_string(),
                position: 0.5
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

    // Read off both halves: while note 63 is down the wheel sends CC 38, not
    // CC 33.
    #[test]
    fn the_wheels_shifted_address_drives_the_same_deck() {
        let profile = ddj_flx6();
        let mut memory = ControlMemory::default();
        for (channel, deck) in [(0, "A"), (1, "B"), (2, "C"), (3, "D")] {
            assert_eq!(
                resolve_move(&profile, &mut memory, &control_change(channel, 38, 70)),
                Some(Move::Jog {
                    deck: deck.to_string(),
                    ticks: 6
                })
            );
        }
    }

    // Read off the hardware: note 63 on each deck's own channel.
    #[test]
    fn shift_resolves_on_both_edges_for_the_channels_own_deck() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        for (channel, deck) in [(0, "A"), (1, "B"), (2, "C"), (3, "D")] {
            assert_eq!(
                resolve_move(&profile, &mut halves, &note_on(channel, 63, 127)),
                Some(Move::Shift {
                    deck: deck.to_string(),
                    held: true
                })
            );
            assert_eq!(
                resolve_move(&profile, &mut halves, &note_on(channel, 63, 0)),
                Some(Move::Shift {
                    deck: deck.to_string(),
                    held: false
                })
            );
        }
    }

    #[test]
    fn each_load_button_names_its_own_deck() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        for (note, deck) in [(70, "A"), (71, "B"), (72, "C"), (73, "D")] {
            assert_eq!(
                resolve_move(&profile, &mut halves, &note_on(6, note, 127)),
                Some(Move::Load {
                    deck: deck.to_string()
                })
            );
            assert_eq!(
                resolve_move(&profile, &mut halves, &note_on(6, note, 0)),
                None
            );
        }
    }

    #[test]
    fn the_browse_wheel_press_and_the_back_button_resolve_to_their_own_moves() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(6, 65, 127)),
            Some(Move::Enter)
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(6, 65, 0)),
            None
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(6, 101, 127)),
            Some(Move::Back)
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(6, 101, 0)),
            None
        );
    }

    #[test]
    fn the_view_button_resolves_to_its_own_move() {
        let profile = ddj_flx6();
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(6, 122, 127)),
            Some(Move::ToggleView)
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(6, 122, 0)),
            None
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
    // Checked against the manifest `apply` resolves through, not a convenient one.
    #[test]
    fn every_binding_addresses_a_real_param() {
        for mapping in built_in_mappings() {
            let profile = mapping.profile(Some("A")).expect(mapping.name());
            for binding in &profile.bindings {
                let (scope, slot, param) = match &binding.action {
                    Action::DeckParam { slot, param, .. } => {
                        (session_core::ParamScope::Deck, slot.as_str(), param.as_str())
                    }
                    Action::XfaderPosition => {
                        (session_core::ParamScope::Master, "xfader", "position")
                    }
                    _ => continue,
                };
                assert!(
                    crate::audio::MIXER.descriptor(scope, slot, param).is_some(),
                    "{}: {slot}/{param}",
                    mapping.name()
                );
            }
        }
    }

    // The crossfader is the address the two shipped manifests disagree about, so a test
    // reading the wrong one would pass while the hardware did nothing.
    #[test]
    fn the_live_mixer_is_the_one_the_crossfader_binding_resolves_against() {
        assert!(crate::audio::MIXER
            .descriptor(session_core::ParamScope::Master, "xfader", "position")
            .is_some());
    }

    // Read off a real failure: the FLX6's output port was held by another client when the
    // input opened, and skipping the retry left every LED dark for the rest of the session.
    #[test]
    fn an_output_that_failed_to_open_is_retried_while_its_input_stays_alone() {
        let wanted = vec!["DDJ-FLX6".to_string(), "XDJ-1000MK2".to_string()];
        let open_inputs = ["DDJ-FLX6"];

        let inputs = ports_to_open(&wanted, |name| open_inputs.contains(&name));
        assert_eq!(inputs, vec!["XDJ-1000MK2".to_string()]);

        // Nothing is open for output, so the port whose input is already up is still tried.
        let outputs = ports_to_open(&wanted, |_| false);
        assert_eq!(outputs, wanted);
    }

    // Every shipped file has to build, or the app ships with a controller that
    // silently does nothing.
    #[test]
    fn every_built_in_mapping_parses_and_builds_a_profile() {
        let mappings = built_in_mappings();
        assert_eq!(mappings.len(), MAPPING_FILES.len());
        for mapping in mappings {
            let assigned = if mapping.needs_deck() {
                Some("A")
            } else {
                None
            };
            mapping.profile(assigned).expect(mapping.name());
        }
    }

    // The file counts from 1 so it can be read next to a capture; the wire counts
    // from 0.
    #[test]
    fn a_files_channel_is_one_higher_than_the_wires() {
        let profile = ddj_flx6();
        assert!(profile
            .resolve(Key::Note {
                channel: 0,
                note: 84
            })
            .is_some());
        assert!(profile
            .resolve(Key::Note {
                channel: 1,
                note: 84
            })
            .is_some());
    }

    #[test]
    fn a_mapping_claims_only_the_port_names_it_names() {
        let flx6 = mapping_named("DDJ-FLX6");
        assert!(flx6.claims("DDJ-FLX6 MIDI 1"));
        assert!(!flx6.claims("XDJ-1000MK2"));
        assert!(!flx6.claims("IAC Driver Bus 1"));

        // The mapping is named for the revision in hand; the substring it claims
        // by is the shorter one both revisions of the port name start with.
        let player = mapping_named("XDJ-1000MK2");
        assert!(player.claims("XDJ-1000MK2"));
        assert!(player.claims("XDJ-1000"));
    }

    // A player is one deck, and which deck is not in the file: it is the user's
    // choice, baked into the profile when they make it.
    #[test]
    fn an_assigned_mapping_names_the_deck_it_was_built_for() {
        let player = mapping_named("XDJ-1000MK2");
        assert!(player.needs_deck());
        let profile = player.profile(Some("D")).expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 0, 127)),
            Some(Move::Play {
                deck: "D".to_string()
            })
        );
    }

    // The bug this fixes: a detented fader that reads a hair above centre moves a
    // 141 bpm track to 141.11 at 10% range, and only a 7-bit control shows it.
    #[test]
    fn a_detented_centre_reads_exactly_half_at_both_resolutions() {
        assert_eq!(unit_interval(64.0, SEVEN_BIT_MAX), 0.5);
        assert_eq!(unit_interval(8192.0, FOURTEEN_BIT_MAX), 0.5);
    }

    #[test]
    fn both_ends_of_a_throw_still_reach_the_ends_of_the_interval() {
        assert_eq!(unit_interval(0.0, SEVEN_BIT_MAX), 0.0);
        assert_eq!(unit_interval(SEVEN_BIT_MAX, SEVEN_BIT_MAX), 1.0);
        assert_eq!(unit_interval(0.0, FOURTEEN_BIT_MAX), 0.0);
        assert_eq!(unit_interval(FOURTEEN_BIT_MAX, FOURTEEN_BIT_MAX), 1.0);
    }

    #[test]
    fn the_players_tempo_fader_centres_on_a_rate_of_one() {
        let profile = mapping_named("XDJ-1000MK2")
            .profile(Some("A"))
            .expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(0, 29, 64)),
            Some(Move::Tempo {
                deck: "A".to_string(),
                position: 0.5
            })
        );
    }

    // The platter and the bend ring are two controls, and only the ring belongs
    // to the jog: the platter is the vinyl gesture and waits for a scratch engine.
    #[test]
    fn the_players_bend_ring_drives_the_jog_and_its_platter_does_not() {
        let profile = mapping_named("XDJ-1000MK2")
            .profile(Some("A"))
            .expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(0, 48, 70)),
            Some(Move::Jog {
                deck: "A".to_string(),
                ticks: 6
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(0, 16, 70)),
            None
        );
    }

    #[test]
    fn an_assigned_mapping_refuses_to_build_without_a_deck() {
        assert!(mapping_named("XDJ-1000MK2").profile(None).is_err());
    }

    // Track search is the browse cursor, not transport: the file gives the step
    // because a button carries no delta of its own.
    #[test]
    fn a_note_can_browse_by_a_fixed_step() {
        let profile = mapping_named("XDJ-1000MK2")
            .profile(Some("A"))
            .expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 4, 127)),
            Some(Move::Browse { steps: 1 })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 5, 127)),
            Some(Move::Browse { steps: -1 })
        );
        assert_eq!(resolve_move(&profile, &mut halves, &note_on(0, 4, 0)), None);
    }

    #[test]
    fn a_file_declaring_a_newer_version_is_refused() {
        let source = r#"{ "version": 99, "name": "Future", "match": [], "decks": "fixed",
                         "bindings": [] }"#;
        assert!(parse_mapping(source).is_err());
    }

    #[test]
    fn a_binding_naming_both_a_note_and_a_control_change_is_refused() {
        let source = r#"{ "version": 1, "name": "Broken", "match": [], "decks": "fixed",
                         "bindings": [{ "channel": 1, "note": 1, "cc": 1, "action": "enter" }] }"#;
        let mapping = parse_mapping(source).expect("a parseable file");
        assert!(mapping.profile(None).is_err());
    }

    #[test]
    fn an_unknown_action_name_is_refused() {
        let source = r#"{ "version": 1, "name": "Broken", "match": [], "decks": "fixed",
                         "bindings": [{ "channel": 1, "note": 1, "action": "teleport" }] }"#;
        let mapping = parse_mapping(source).expect("a parseable file");
        assert!(mapping.profile(None).is_err());
    }

    // The LED path is a reverse lookup, so a cue binding the profile can dispatch
    // but not light would leave the button dark whatever the app did.
    #[test]
    fn every_cue_binding_has_a_key_to_light() {
        let profile = ddj_flx6();
        for deck in ["A", "B", "C", "D"] {
            assert!(
                profile
                    .led_keys
                    .contains_key(&(Feedback::Cue, deck.to_string())),
                "{deck}"
            );
        }
    }

    // Cue is note 84 and quantize note 48 on the same deck.
    #[test]
    fn each_kind_of_feedback_keeps_its_own_key() {
        let profile = Profile::new(vec![
            Binding {
                source: Source::Note {
                    channel: 0,
                    note: 84,
                },
                action: Action::CueToggle {
                    deck: "A".to_string(),
                },
            },
            Binding {
                source: Source::Note {
                    channel: 0,
                    note: 48,
                },
                action: Action::QuantizeToggle {
                    deck: "A".to_string(),
                },
            },
        ])
        .expect("a profile");
        assert_eq!(
            profile.led_keys.get(&(Feedback::Cue, "A".to_string())),
            Some(&Key::Note {
                channel: 0,
                note: 84
            })
        );
        assert_eq!(
            profile.led_keys.get(&(Feedback::Quantize, "A".to_string())),
            Some(&Key::Note {
                channel: 0,
                note: 48
            })
        );
    }

    // Momentary on the surface, latching in the app: acting on the release too
    // would turn quantize straight back off under the finger.
    #[test]
    fn quantize_turns_over_on_the_press_only() {
        let profile = mapping_named("XDJ-1000MK2")
            .profile(Some("A"))
            .expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 9, 127)),
            Some(Move::QuantizeToggle {
                deck: "A".to_string()
            })
        );
        assert_eq!(resolve_move(&profile, &mut halves, &note_on(0, 9, 0)), None);
        assert_eq!(
            profile.led_keys.get(&(Feedback::Quantize, "A".to_string())),
            Some(&Key::Note {
                channel: 0,
                note: 9
            })
        );
    }

    #[test]
    fn the_players_eject_button_names_its_assigned_deck() {
        let profile = mapping_named("XDJ-1000MK2")
            .profile(Some("C"))
            .expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 48, 127)),
            Some(Move::Eject {
                deck: "C".to_string()
            })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 48, 0)),
            None
        );
    }

    // Read off the unit: the rotary sends 127 one way and 1 the other.
    #[test]
    fn the_players_browse_encoder_steps_both_ways() {
        let profile = mapping_named("XDJ-1000MK2")
            .profile(Some("A"))
            .expect("the player mapping");
        let mut halves = ControlMemory::default();
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(0, 79, 127)),
            Some(Move::Browse { steps: -1 })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &control_change(0, 79, 1)),
            Some(Move::Browse { steps: 1 })
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 51, 127)),
            Some(Move::Enter)
        );
        assert_eq!(
            resolve_move(&profile, &mut halves, &note_on(0, 50, 127)),
            Some(Move::Back)
        );
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
