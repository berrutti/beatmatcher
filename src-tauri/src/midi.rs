use std::collections::VecDeque;
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
// High-resolution control change puts the low half on the controller 32 above
// the high half's.
const LSB_OFFSET: u8 = 32;
const SEVEN_BIT_MAX: f64 = 127.0;
const FOURTEEN_BIT_MAX: f64 = 16383.0;

// Scaffolding until mapping profiles land. Read off a DDJ-FLX6 rather than
// guessed: every control it sends is high resolution, each deck owns a channel
// for its eq and fader, and the mixer's filter knobs share one channel.
const BINDINGS: &[Binding] = &[
    Binding::high_resolution(0, 7, "A", "eq", "high"),
    Binding::high_resolution(0, 11, "A", "eq", "mid"),
    Binding::high_resolution(0, 15, "A", "eq", "low"),
    Binding::high_resolution(0, 19, "A", "fader", "gain"),
    Binding::high_resolution(1, 7, "B", "eq", "high"),
    Binding::high_resolution(1, 11, "B", "eq", "mid"),
    Binding::high_resolution(1, 15, "B", "eq", "low"),
    Binding::high_resolution(1, 19, "B", "fader", "gain"),
    Binding::high_resolution(2, 7, "C", "eq", "high"),
    Binding::high_resolution(2, 11, "C", "eq", "mid"),
    Binding::high_resolution(2, 15, "C", "eq", "low"),
    Binding::high_resolution(2, 19, "C", "fader", "gain"),
    Binding::high_resolution(3, 7, "D", "eq", "high"),
    Binding::high_resolution(3, 11, "D", "eq", "mid"),
    Binding::high_resolution(3, 15, "D", "eq", "low"),
    Binding::high_resolution(3, 19, "D", "fader", "gain"),
    Binding::high_resolution(6, 23, "A", "filter", "value"),
    Binding::high_resolution(6, 24, "B", "filter", "value"),
    Binding::high_resolution(6, 25, "C", "filter", "value"),
    Binding::high_resolution(6, 26, "D", "filter", "value"),
];

struct Binding {
    channel: u8,
    // For a high-resolution binding this is the high half; the low half is
    // implied at LSB_OFFSET above it.
    controller: u8,
    high_resolution: bool,
    deck: &'static str,
    slot: &'static str,
    param: &'static str,
}

impl Binding {
    const fn high_resolution(
        channel: u8,
        controller: u8,
        deck: &'static str,
        slot: &'static str,
        param: &'static str,
    ) -> Self {
        Self {
            channel,
            controller,
            high_resolution: true,
            deck,
            slot,
            param,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Half {
    Msb,
    Lsb,
}

#[derive(Default)]
struct HighResolution {
    halves: std::collections::HashMap<(u8, u8), (u8, u8)>,
}

impl HighResolution {
    /// Acts on both halves instead of waiting for a pair to complete: a
    /// controller that sends only the half that changed would otherwise stall
    /// forever. The intermediate that a high half produces on its own is at
    /// most one low-half step away from the value the next message brings, so
    /// it is corrected before it can be heard.
    fn join(&mut self, channel: u8, controller: u8, half: Half, value: u8) -> u16 {
        let halves = self.halves.entry((channel, controller)).or_insert((0, 0));
        match half {
            Half::Msb => halves.0 = value,
            Half::Lsb => halves.1 = value,
        }
        (u16::from(halves.0) << 7) | u16::from(halves.1)
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

fn resolve(message: &ControlChange) -> Option<(&'static Binding, Half)> {
    BINDINGS.iter().find_map(|binding| {
        if binding.channel != message.channel {
            return None;
        }
        if binding.controller == message.controller {
            return Some((binding, Half::Msb));
        }
        let low = binding.controller.checked_add(LSB_OFFSET);
        if binding.high_resolution && low == Some(message.controller) {
            return Some((binding, Half::Lsb));
        }
        None
    })
}

fn position(
    halves: &mut HighResolution,
    message: &ControlChange,
    binding: &Binding,
    half: Half,
) -> f64 {
    if !binding.high_resolution {
        return f64::from(message.value) / SEVEN_BIT_MAX;
    }
    let joined = halves.join(message.channel, binding.controller, half, message.value);
    f64::from(joined) / FOURTEEN_BIT_MAX
}

const NOTE_ON: u8 = 0x90;

// Note 84 on the deck's own channel, with the release arriving as the same note
// at zero velocity.
const CUE_BINDINGS: &[CueBinding] = &[
    CueBinding::new(0, 84, "A"),
    CueBinding::new(1, 84, "B"),
    CueBinding::new(2, 84, "C"),
    CueBinding::new(3, 84, "D"),
];

struct CueBinding {
    channel: u8,
    note: u8,
    deck: &'static str,
}

impl CueBinding {
    const fn new(channel: u8, note: u8, deck: &'static str) -> Self {
        Self {
            channel,
            note,
            deck,
        }
    }
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

pub(crate) fn apply(state: &crate::AppState, data: &[u8]) {
    if let Some(message) = parse_control_change(data) {
        apply_control_change(state, &message);
    } else if let Some(message) = parse_note_on(data) {
        apply_note_on(state, &message);
    }
}

/// Lights a deck's cue button to match the app. Driven by every cue change
/// whatever caused it, so a mouse toggle lights the button too.
pub fn send_cue_led(state: &MidiState, deck: &str, active: bool) {
    let Some(binding) = CUE_BINDINGS.iter().find(|binding| binding.deck == deck) else {
        return;
    };
    state.send(Request::Send(vec![
        NOTE_ON | binding.channel,
        binding.note,
        if active { 127 } else { 0 },
    ]));
}

// Cue is a toggle in the app but a momentary button on the controller, so only
// the press acts. Acting on the release too would undo it immediately.
fn apply_note_on(state: &crate::AppState, message: &NoteOn) {
    if message.velocity == 0 {
        return;
    }
    let Some(binding) = CUE_BINDINGS
        .iter()
        .find(|binding| binding.channel == message.channel && binding.note == message.note)
    else {
        return;
    };
    state
        .toggle_cue_active(crate::ParamOrigin::Midi, binding.deck)
        .ok();
}

fn apply_control_change(state: &crate::AppState, message: &ControlChange) {
    static HALVES: Mutex<Option<HighResolution>> = Mutex::new(None);

    let Some((binding, half)) = resolve(message) else {
        return;
    };
    let control_position = {
        let mut guard = HALVES.lock().unwrap_or_else(|error| error.into_inner());
        position(
            guard.get_or_insert_with(HighResolution::default),
            message,
            binding,
            half,
        )
    };
    let Some(descriptor) =
        state
            .audio
            .mixer()
            .descriptor(session_core::ParamScope::Deck, binding.slot, binding.param)
    else {
        return;
    };
    let value = descriptor.from_unit_interval(control_position);
    state
        .set_deck_param(
            crate::ParamOrigin::Midi,
            binding.deck,
            binding.slot,
            binding.param,
            value as f32,
        )
        .ok();
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
    port: Option<String>,
) -> Result<(), String> {
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

    #[test]
    fn both_halves_of_a_bound_control_resolve_and_an_unbound_one_does_not() {
        let high = parse_control_change(&control_change(0, 15, 64)).expect("a control change");
        let (binding, half) = resolve(&high).expect("cc 15 is bound");
        assert_eq!(
            (binding.deck, binding.slot, binding.param),
            ("A", "eq", "low")
        );
        assert_eq!(half, Half::Msb);

        let low = parse_control_change(&control_change(0, 47, 0)).expect("a control change");
        let (_, half) = resolve(&low).expect("cc 47 is the low half of cc 15");
        assert_eq!(half, Half::Lsb);

        let unbound = parse_control_change(&control_change(0, 99, 0)).expect("a control change");
        assert!(resolve(&unbound).is_none());
        // The low half exists only on the binding's own channel.
        let elsewhere = parse_control_change(&control_change(5, 47, 0)).expect("a control change");
        assert!(resolve(&elsewhere).is_none());
    }

    // Each deck owns a channel and each control a CC, so a crossed pair would
    // silently move the wrong deck's knob.
    #[test]
    fn no_two_bindings_share_an_address() {
        for (index, binding) in BINDINGS.iter().enumerate() {
            for other in &BINDINGS[index + 1..] {
                assert!(
                    binding.channel != other.channel || binding.controller != other.controller,
                    "{}/{} collides with {}/{}",
                    binding.deck,
                    binding.param,
                    other.deck,
                    other.param
                );
            }
        }
    }

    // The exact stream from the screenshot, high half first, ending at the 8192
    // centre of the 14-bit range.
    #[test]
    fn the_two_halves_of_a_high_resolution_control_join() {
        let mut halves = HighResolution::default();
        assert_eq!(halves.join(0, 15, Half::Msb, 61), 61 << 7);
        assert_eq!(halves.join(0, 15, Half::Lsb, 75), (61 << 7) | 75);
        assert_eq!(halves.join(0, 15, Half::Msb, 62), (62 << 7) | 75);
        assert_eq!(halves.join(0, 15, Half::Lsb, 50), (62 << 7) | 50);
        assert_eq!(halves.join(0, 15, Half::Msb, 64), (64 << 7) | 50);
        assert_eq!(halves.join(0, 15, Half::Lsb, 0), 8192);
    }

    // Acting on a lone high half is what stops a controller that sends only the
    // half that moved from stalling, and it has to land within one low-half step.
    #[test]
    fn a_high_half_on_its_own_lands_within_one_low_half_step() {
        let mut halves = HighResolution::default();
        halves.join(0, 15, Half::Msb, 61);
        halves.join(0, 15, Half::Lsb, 127);
        let intermediate = halves.join(0, 15, Half::Msb, 62);
        let settled = halves.join(0, 15, Half::Lsb, 0);
        assert!(u32::from(intermediate).abs_diff(u32::from(settled)) <= 127);
    }

    #[test]
    fn two_high_resolution_controls_do_not_share_halves() {
        let mut halves = HighResolution::default();
        halves.join(0, 15, Half::Msb, 64);
        assert_eq!(halves.join(1, 15, Half::Lsb, 3), 3);
    }

    #[test]
    fn a_high_resolution_control_spans_the_whole_param_range() {
        let mut halves = HighResolution::default();
        let binding = &BINDINGS[0];
        let at = |controller: u8, value: u8| ControlChange {
            channel: 0,
            controller,
            value,
        };

        assert_eq!(position(&mut halves, &at(47, 0), binding, Half::Lsb), 0.0);
        position(&mut halves, &at(15, 127), binding, Half::Msb);
        assert_eq!(position(&mut halves, &at(47, 127), binding, Half::Lsb), 1.0);
    }

    // A binding naming an address the mixer does not have would silently do
    // nothing at runtime, which is indistinguishable from a broken controller.
    #[test]
    fn every_binding_addresses_a_real_param() {
        for binding in BINDINGS {
            assert!(
                session_core::CLASSIC_3BAND
                    .descriptor(session_core::ParamScope::Deck, binding.slot, binding.param)
                    .is_some(),
                "{}/{}",
                binding.slot,
                binding.param
            );
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
