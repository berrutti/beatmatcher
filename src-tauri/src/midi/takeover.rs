use std::time::{Duration, Instant};

const THRESHOLD: f64 = 3.0 / 128.0;

// Once caught, a fast sweep moves further than the threshold between messages.
const FOLLOW_WINDOW: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Target {
    Param {
        deck: String,
        slot: String,
        param: String,
    },
    Xfader,
}

#[derive(Default)]
pub(super) struct SoftTakeover {
    previous: Option<f64>,
    accepted_at: Option<Instant>,
}

impl SoftTakeover {
    /// The first position is ignored, because without a previous one nothing says
    /// whether a knob far from the value has crossed it or would jump it.
    pub(super) fn ignores(&mut self, current: f64, incoming: f64, now: Instant) -> bool {
        let ignored = match self.previous {
            None => true,
            Some(previous) => self.holds_off(current, incoming, previous, now),
        };
        if !ignored {
            self.accepted_at = Some(now);
        }
        self.previous = Some(incoming);
        ignored
    }

    fn holds_off(&self, current: f64, incoming: f64, previous: f64, now: Instant) -> bool {
        if self
            .accepted_at
            .is_some_and(|accepted| now < accepted + FOLLOW_WINDOW)
        {
            return false;
        }
        let distance = current - incoming;
        let previous_distance = current - previous;
        if distance.is_sign_negative() != previous_distance.is_sign_negative() {
            return false;
        }
        distance.abs() > THRESHOLD && previous_distance.abs() > THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATER: Duration = Duration::from_millis(200);

    #[test]
    fn the_first_position_is_ignored_even_on_the_value() {
        let mut takeover = SoftTakeover::default();
        assert!(takeover.ignores(0.5, 0.5, Instant::now()));
    }

    #[test]
    fn a_knob_far_from_the_value_is_held_off_until_it_crosses_it() {
        let mut takeover = SoftTakeover::default();
        let start = Instant::now();
        assert!(takeover.ignores(0.5, 0.1, start));
        assert!(takeover.ignores(0.5, 0.3, start + LATER));
        assert!(!takeover.ignores(0.5, 0.6, start + LATER * 2));
    }

    #[test]
    fn a_knob_arriving_within_the_threshold_takes_over() {
        let mut takeover = SoftTakeover::default();
        let start = Instant::now();
        assert!(takeover.ignores(0.5, 0.1, start));
        assert!(!takeover.ignores(0.5, 0.5 - THRESHOLD, start + LATER));
    }

    #[test]
    fn a_caught_knob_is_followed_through_a_fast_sweep() {
        let mut takeover = SoftTakeover::default();
        let start = Instant::now();
        takeover.ignores(0.5, 0.49, start);
        assert!(!takeover.ignores(0.5, 0.5, start + LATER));
        assert!(!takeover.ignores(0.5, 0.9, start + LATER + FOLLOW_WINDOW / 2));
    }

    #[test]
    fn a_value_moved_elsewhere_while_the_knob_rests_holds_the_knob_off_again() {
        let mut takeover = SoftTakeover::default();
        let start = Instant::now();
        takeover.ignores(0.5, 0.49, start);
        assert!(!takeover.ignores(0.5, 0.5, start + LATER));
        assert!(takeover.ignores(0.9, 0.52, start + LATER * 2));
    }
}
