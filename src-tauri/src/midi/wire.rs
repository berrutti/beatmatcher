pub(super) const CONTROL_CHANGE: u8 = 0xB0;

pub(super) const NOTE_ON: u8 = 0x90;

// High-resolution control change puts the low half on the controller 32 above
// the high half's.
pub(super) const LSB_OFFSET: u8 = 32;

pub(super) const MAX_CONTROLLER: u8 = 127;

pub(super) const SEVEN_BIT_MAX: f64 = 127.0;

pub(super) const FOURTEEN_BIT_MAX: f64 = 16383.0;

pub(super) const RELATIVE_CENTRE: i32 = 64;

pub(super) const SEVEN_BIT_WRAP: i32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Resolution {
    SevenBit,
    FourteenBit,
    // Read off the platter: a steady turn holds one value just off
    // `RELATIVE_CENTRE` and a faster one sits further off, so it reports speed.
    CentreDelta,
    // The browse encoder: 1 or 127, one detent either way.
    SignedStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Half {
    Msb,
    Lsb,
}

/// A high-resolution control names only its high half; the low half is implied
/// at `LSB_OFFSET` above it.
pub(super) enum Source {
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
    pub(super) fn keys(&self) -> Result<Vec<(Key, Half)>, String> {
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

    pub(super) fn resolution(&self) -> Resolution {
        match *self {
            Source::ControlChange { resolution, .. } => resolution,
            Source::Note { .. } => Resolution::SevenBit,
        }
    }
}

/// A control cannot send its own midpoint (63.5 of 7 bits), so a plain `value / max` puts
/// a detent past half. On a 7-bit tempo fader at 10% that showed 141 bpm as 141.11.
pub(super) fn unit_interval(value: f64, max: f64) -> f64 {
    let centre = (max + 1.0) / 2.0;
    if value <= centre {
        value / (centre * 2.0)
    } else {
        0.5 + (value - centre) / (max - centre) / 2.0
    }
}

/// `None` for anything that reports a position, which has to be read through the
/// binding's range instead.
pub(super) fn relative_delta(resolution: Resolution, value: u8) -> Option<i32> {
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

/// Distinct from `Source` because a high-resolution control declares one source
/// but arrives on two addresses, and dispatch has to find it by either.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Key {
    ControlChange { channel: u8, controller: u8 },
    Note { channel: u8, note: u8 },
}
