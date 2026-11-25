//! Decides when to warn about a low headset battery: once at 20 %, once more
//! at 10 %, and again only after the headset was charged.

/// Warning to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alert {
    Low(u8),
    Critical(u8),
}

pub const LOW: u8 = 20;
pub const CRITICAL: u8 = 10;
/// Above this (or while charging) the warnings re-arm.
const REARM: u8 = 25;

#[derive(Clone, Copy, Debug, Default)]
pub struct LowBatteryAlert {
    warned_low: bool,
    warned_critical: bool,
}

impl LowBatteryAlert {
    /// Feed the current level; returns a warning when one is due.
    pub fn update(&mut self, level: u8, charging: bool) -> Option<Alert> {
        if charging || level > REARM {
            *self = Self::default();
            return None;
        }
        if level <= CRITICAL && !self.warned_critical {
            self.warned_critical = true;
            self.warned_low = true;
            return Some(Alert::Critical(level));
        }
        if level <= LOW && !self.warned_low {
            self.warned_low = true;
            return Some(Alert::Low(level));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_per_threshold() {
        let mut a = LowBatteryAlert::default();
        assert_eq!(a.update(50, false), None);
        assert_eq!(a.update(20, false), Some(Alert::Low(20)));
        assert_eq!(a.update(19, false), None);
        assert_eq!(a.update(15, false), None);
        assert_eq!(a.update(10, false), Some(Alert::Critical(10)));
        assert_eq!(a.update(5, false), None);
    }

    #[test]
    fn rearms_after_charging_and_skips_low_when_already_critical() {
        let mut a = LowBatteryAlert::default();
        assert_eq!(a.update(8, false), Some(Alert::Critical(8)));
        assert_eq!(a.update(18, false), None); // still within the same discharge
        assert_eq!(a.update(30, true), None); // charging re-arms
        assert_eq!(a.update(19, false), Some(Alert::Low(19)));
        // Small fluctuations around the threshold don't repeat the warning.
        assert_eq!(a.update(21, false), None);
        assert_eq!(a.update(20, false), None);
    }
}
