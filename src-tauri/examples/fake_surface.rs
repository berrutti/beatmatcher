//! A virtual MIDI surface, so the learn panel can be exercised with no hardware.
//! `cargo run --example fake_surface` from `src-tauri`, then open Settings and the
//! port appears alongside any real device.

use midir::os::unix::VirtualOutput;
use midir::MidiOutput;
use std::io::{BufRead, Write};
use std::thread::sleep;
use std::time::Duration;

/// Claimed by no shipped mapping, so the panel opens on an empty draft.
const PORT_NAME: &str = "beatmatcher fake surface";

const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;

/// Well under the learn window's settle, so a whole gesture lands as one capture.
const STEP: Duration = Duration::from_millis(12);

struct Surface {
    port: midir::MidiOutputConnection,
}

impl Surface {
    fn send(&mut self, message: &[u8]) {
        if let Err(error) = self.port.send(message) {
            eprintln!("send failed: {error}");
        }
        sleep(STEP);
    }

    fn press_and_release(&mut self, channel: u8, note: u8) {
        self.send(&[NOTE_ON | channel, note, 127]);
        self.send(&[NOTE_ON | channel, note, 0]);
    }

    /// A pad that reports nothing when it is let go.
    fn press_only(&mut self, channel: u8, note: u8) {
        self.send(&[NOTE_ON | channel, note, 127]);
    }

    fn sweep_7bit(&mut self, channel: u8, controller: u8) {
        for step in 0..=16 {
            self.send(&[CONTROL_CHANGE | channel, controller, step * 7]);
        }
    }

    /// The high half moves on the carries only, which is what the hardware does and
    /// what the capture has to read the pair from.
    fn sweep_14bit(&mut self, channel: u8, high: u8) {
        for step in 0..=32u16 {
            let value = step * 511;
            let msb = (value >> 7) as u8;
            let lsb = (value & 0x7F) as u8;
            self.send(&[CONTROL_CHANGE | channel, high, msb]);
            self.send(&[CONTROL_CHANGE | channel, high + 32, lsb]);
        }
    }

    /// A steady turn sits just off centre the whole way, never sweeping the range.
    fn platter(&mut self, channel: u8, controller: u8) {
        for value in [65, 66, 67, 67, 66, 65, 63, 62, 61, 62, 64] {
            self.send(&[CONTROL_CHANGE | channel, controller, value]);
        }
    }

    fn encoder(&mut self, channel: u8, controller: u8) {
        for value in [1, 1, 127, 1, 127, 127] {
            self.send(&[CONTROL_CHANGE | channel, controller, value]);
        }
    }
}

fn menu() {
    println!();
    println!("  1  button, press and release   (note 11, ch 1)");
    println!("  2  button, press only          (note 12, ch 1)");
    println!("  3  fader, 14-bit               (cc 7 + 39, ch 1)");
    println!("  4  fader, 7-bit                (cc 20, ch 1)");
    println!("  5  jog platter                 (cc 33, ch 1)");
    println!("  6  browse encoder              (cc 64, ch 7)");
    println!("  7  a second button             (note 84, ch 1)");
    println!("  q  quit");
    print!("> ");
    std::io::stdout().flush().ok();
}

fn main() {
    let output = MidiOutput::new("beatmatcher fake surface").expect("a MIDI client");
    let port = output
        .create_virtual(PORT_NAME)
        .expect("a virtual MIDI port");
    let mut surface = Surface { port };

    println!("'{PORT_NAME}' is open. Rescan in Settings to see it.");

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        menu();
        let Some(Ok(line)) = lines.next() else { break };
        match line.trim() {
            "1" => surface.press_and_release(0, 11),
            "2" => surface.press_only(0, 12),
            "3" => surface.sweep_14bit(0, 7),
            "4" => surface.sweep_7bit(0, 20),
            "5" => surface.platter(0, 33),
            "6" => surface.encoder(6, 64),
            "7" => surface.press_and_release(0, 84),
            "q" => break,
            other => println!("no such control: '{other}'"),
        }
    }
}
