//! Brown-Conrady lens model in normalized image coordinates.
//! Coefficients require calibration with a real target; defaults are identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    pub k1: f64,
    pub k2: f64,
    pub p1: f64,
    pub p2: f64,
    pub cx: f64,
    pub cy: f64,
    pub fx: f64,
    pub fy: f64,
}
impl Default for Lens {
    fn default() -> Self {
        Self { k1: 0.0, k2: 0.0, p1: 0.0, p2: 0.0, cx: 0.5, cy: 0.5, fx: 1.0, fy: 1.0 }
    }
}
impl Lens {
    pub fn valid(&self) -> bool {
        [self.k1, self.k2, self.p1, self.p2, self.cx, self.cy, self.fx, self.fy].iter().all(|v| v.is_finite()) && self.fx > 0.0 && self.fy > 0.0
    }
    pub fn distort(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        if !self.valid() {
            return None;
        }
        let x = (u - self.cx) / self.fx;
        let y = (v - self.cy) / self.fy;
        let r2 = x * x + y * y;
        let radial = 1.0 + self.k1 * r2 + self.k2 * r2 * r2;
        let xd = x * radial + 2.0 * self.p1 * x * y + self.p2 * (r2 + 2.0 * x * x);
        let yd = y * radial + self.p1 * (r2 + 2.0 * y * y) + 2.0 * self.p2 * x * y;
        let (a, b) = (xd * self.fx + self.cx, yd * self.fy + self.cy);
        if a.is_finite() && b.is_finite() { Some((a, b)) } else { None }
    }
    /// Iterative inverse, rejecting nonconvergent lens parameters.
    pub fn undistort(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        if !self.valid() {
            return None;
        }
        let (mut x, mut y) = (u, v);
        for _ in 0..30 {
            let (a, b) = self.distort(x, y)?;
            let (dx, dy) = (u - a, v - b);
            x += dx;
            y += dy;
            if dx.abs().max(dy.abs()) < 1e-10 {
                return Some((x, y));
            }
            if !x.is_finite() || !y.is_finite() || x.abs() > 100.0 || y.abs() > 100.0 {
                return None;
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity() {
        let l = Lens::default();
        assert_eq!(l.distort(0.3, 0.7), Some((0.3, 0.7)));
    }
    #[test]
    fn round_trip() {
        let l = Lens { k1: -0.16, k2: 0.02, p1: 0.001, p2: -0.002, ..Lens::default() };
        let p = l.distort(0.2, 0.8).unwrap();
        let q = l.undistort(p.0, p.1).unwrap();
        assert!((q.0 - 0.2).abs() < 1e-7 && (q.1 - 0.8).abs() < 1e-7);
    }
    #[test]
    fn invalid() {
        let l = Lens { fx: 0.0, ..Lens::default() };
        assert!(l.distort(0.5, 0.5).is_none());
    }
}
