//! GRBL's `$` settings, as a `$$` report lists them.

use std::collections::BTreeMap;

/// The settings a controller reported (`$N=value`), with the ones LaserCraft checks named.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub values: BTreeMap<u16, f64>,
}

impl Settings {
    /// Most settings kept (GRBL has about 35; a flood of lines is not a settings report).
    const MAX: usize = 256;

    pub fn set(&mut self, id: u16, value: f64) {
        if self.values.len() < Self::MAX || self.values.contains_key(&id) {
            self.values.insert(id, value);
        }
    }
    pub fn get(&self, id: u16) -> Option<f64> {
        self.values.get(&id).copied()
    }
    /// `$32`: laser mode (the laser turns off between moves and follows the feed).
    pub fn laser_mode(&self) -> Option<bool> {
        self.get(32).map(|v| v != 0.0)
    }
    /// `$30`: the S value of full power.
    pub fn max_spindle(&self) -> Option<f64> {
        self.get(30)
    }
    /// `$130`, `$131`: the X and Y travel (mm).
    pub fn travel(&self) -> Option<(f64, f64)> {
        Some((self.get(130)?, self.get(131)?))
    }
    /// `$110`, `$111`: the X and Y maximum rates (mm/min).
    pub fn max_rate(&self) -> Option<(f64, f64)> {
        Some((self.get(110)?, self.get(111)?))
    }
    /// `$120`, `$121`: the X and Y acceleration (mm/s²).
    pub fn acceleration(&self) -> Option<(f64, f64)> {
        Some((self.get(120)?, self.get(121)?))
    }
    /// `$22`: homing enabled.
    pub fn homing(&self) -> Option<bool> {
        self.get(22).map(|v| v != 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_settings() {
        let mut s = Settings::default();
        assert_eq!((s.laser_mode(), s.travel()), (None, None));
        for (id, v) in [(32, 1.0), (30, 1000.0), (130, 400.0), (131, 300.0), (110, 6000.0), (111, 5000.0), (120, 500.0), (121, 400.0), (22, 1.0)] {
            s.set(id, v);
        }
        assert_eq!(s.laser_mode(), Some(true));
        assert_eq!(s.max_spindle(), Some(1000.0));
        assert_eq!(s.travel(), Some((400.0, 300.0)));
        assert_eq!(s.max_rate(), Some((6000.0, 5000.0)));
        assert_eq!(s.acceleration(), Some((500.0, 400.0)));
        assert_eq!(s.homing(), Some(true));
        for id in 1000..2000 {
            s.set(id, 1.0);
        }
        assert_eq!(s.values.len(), Settings::MAX, "capped");
        s.set(32, 0.0);
        assert_eq!(s.laser_mode(), Some(false), "known ids still update");
    }
}
