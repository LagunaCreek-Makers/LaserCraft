//! Offline camera-to-bed planar homography. Coordinates are normalized camera pixels.
//! Does not compensate lens distortion or workpiece height/parallax.
#[derive(Clone, Debug)]
pub struct Homography {
    pub h: [f64; 9],
}
impl Homography {
    pub fn from_bed_corners(corners: &[[f32; 2]; 4], width: f64, height: f64) -> Result<Self, String> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("Invalid bed dimensions".into());
        }
        let source = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]];
        let mut a = [[0.0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (source[i][0], source[i][1]);
            let (u, v) = (corners[i][0] as f64, corners[i][1] as f64);
            if !u.is_finite() || !v.is_finite() || !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                return Err("Invalid camera corner".into());
            }
            a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        for k in 0..8 {
            let pivot = (k..8).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs())).unwrap();
            if a[pivot][k].abs() < 1e-12 {
                return Err("Degenerate corner geometry".into());
            }
            a.swap(k, pivot);
            let divisor = a[k][k];
            for j in k..9 {
                a[k][j] /= divisor;
            }
            for i in 0..8 {
                if i != k {
                    let f = a[i][k];
                    for j in k..9 {
                        a[i][j] -= f * a[k][j];
                    }
                }
            }
        }
        let mut h = [0.0; 9];
        for i in 0..8 {
            h[i] = a[i][8];
        }
        h[8] = 1.0;
        let result = Self { h };
        for (i, p) in source.iter().enumerate() {
            let q = result.project(p[0], p[1]).ok_or("Invalid projective mapping")?;
            if (q.0 - corners[i][0] as f64).abs() > 1e-4 || (q.1 - corners[i][1] as f64).abs() > 1e-4 {
                return Err("Unstable calibration".into());
            }
        }
        Ok(result)
    }
    /// Inverse camera mapping for interactive placement. Returns physical bed millimeters.
    pub fn unproject(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        let h = &self.h;
        let a = h[0] - u * h[6];
        let b = h[1] - u * h[7];
        let c = u - h[2];
        let d = h[3] - v * h[6];
        let e = h[4] - v * h[7];
        let f = v - h[5];
        let det = a * e - b * d;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let x = (c * e - b * f) / det;
        let y = (a * f - c * d) / det;
        if x.is_finite() && y.is_finite() { Some((x, y)) } else { None }
    }
    pub fn project(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let d = self.h[6] * x + self.h[7] * y + self.h[8];
        if d.abs() < 1e-12 {
            return None;
        }
        let u = (self.h[0] * x + self.h[1] * y + self.h[2]) / d;
        let v = (self.h[3] * x + self.h[4] * y + self.h[5]) / d;
        if u.is_finite() && v.is_finite() { Some((u, v)) } else { None }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perspective_corners() {
        let c = [[0.1, 0.9], [0.9, 0.8], [0.75, 0.1], [0.2, 0.2]];
        let h = Homography::from_bed_corners(&c, 400.0, 300.0).unwrap();
        for (i, (x, y)) in [(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)].iter().enumerate() {
            let q = h.project(*x, *y).unwrap();
            assert!((q.0 - c[i][0] as f64).abs() > 1e-5);
            assert!((q.1 - c[i][1] as f64).abs() > 1e-5);
        }
    }
    #[test]
    fn inverse_round_trip() {
        let c = [[0.1, 0.9], [0.9, 0.8], [0.75, 0.1], [0.2, 0.2]];
        let h = Homography::from_bed_corners(&c, 400.0, 300.0).unwrap();
        let (u, v) = h.project(173.0, 121.0).unwrap();
        let (x, y) = h.unproject(u, v).unwrap();
        assert!((x - 173.0).abs() < 1e-7 && (y - 121.0).abs() < 1e-7);
    }
    #[test]
    fn rejects_bad() {
        assert!(Homography::from_bed_corners(&[[0.0, 0.0]; 4], 300.0, 200.0).is_err());
    }
}
