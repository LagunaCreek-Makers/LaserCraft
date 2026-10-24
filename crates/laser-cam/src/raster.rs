//! Image mode: placed images engraved line by line.
//!
//! An image is resampled at the layer's line interval along its own rows and columns (so a turned
//! image is engraved along its turned rows), each sample the area average of the pixels under it
//! (`ImageBlob::grid_samples`). Darkness becomes laser power: dithered to full power or none
//! (threshold, ordered, Floyd–Steinberg or Jarvis–Judice–Ninke error diffusion), or kept as grey
//! levels between the minimum and maximum power. Transparent pixels never burn.

use serde::{Deserialize, Serialize};

/// Most samples (pixels at the line interval) one image may have: a cap on the work and memory a
/// fine interval on a big image asks for.
pub const MAX_SAMPLES: u64 = 16_000_000;
/// Most samples along one side.
pub const MAX_SIDE: u64 = 40_000;

/// How grey levels become laser power.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dither {
    /// Darker than half: full power, else none.
    Threshold,
    /// A 4 × 4 Bayer pattern of thresholds.
    Ordered,
    /// Error diffusion to four neighbours (Floyd–Steinberg weights).
    #[default]
    FloydSteinberg,
    /// Error diffusion to twelve neighbours (Jarvis, Judice and Ninke weights).
    Jarvis,
    /// No dithering: each pixel's darkness sets the power between the minimum and the maximum.
    Grayscale,
}

impl Dither {
    pub const ALL: [Dither; 5] = [Dither::Threshold, Dither::Ordered, Dither::FloydSteinberg, Dither::Jarvis, Dither::Grayscale];
    pub fn id(self) -> &'static str {
        match self {
            Dither::Threshold => "threshold",
            Dither::Ordered => "ordered",
            Dither::FloydSteinberg => "floydSteinberg",
            Dither::Jarvis => "jarvis",
            Dither::Grayscale => "grayscale",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Dither::Threshold => "Threshold",
            Dither::Ordered => "Ordered",
            Dither::FloydSteinberg => "Floyd–Steinberg",
            Dither::Jarvis => "Jarvis",
            Dither::Grayscale => "Grayscale",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        let k = s.trim().to_ascii_lowercase().replace(['-', '_', ' ', '–'], "");
        Self::ALL.into_iter().find(|d| d.id().to_ascii_lowercase() == k)
    }
}

/// The darkness (0: no burn, 1: full) of RGBA samples: luminance, inverted for a negative,
/// transparent samples 0 either way.
pub(crate) fn darkness(samples: &[[u8; 4]], negative: bool) -> Vec<f32> {
    samples
        .iter()
        .map(|&[r, g, b, a]| {
            let lum = (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0;
            let d = if negative { lum } else { 1.0 - lum };
            // Partly transparent: as if over white (no burn).
            d * f32::from(a) / 255.0
        })
        .collect()
}

/// The burn level (0..=1) of each of `w` × `h` darkness values (row-major, row 0 first) with
/// `dither`: 0 or 1 when dithered, the darkness itself for Grayscale.
pub(crate) fn levels(dark: &[f32], w: usize, h: usize, dither: Dither) -> Vec<f32> {
    const BAYER: [[f32; 4]; 4] = [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];
    // (dx, dy, weight) and the weights' sum.
    const FS: (&[(isize, isize, f32)], f32) = (&[(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)], 16.0);
    const JJN: (&[(isize, isize, f32)], f32) = (
        &[
            (1, 0, 7.0),
            (2, 0, 5.0),
            (-2, 1, 3.0),
            (-1, 1, 5.0),
            (0, 1, 7.0),
            (1, 1, 5.0),
            (2, 1, 3.0),
            (-2, 2, 1.0),
            (-1, 2, 3.0),
            (0, 2, 5.0),
            (1, 2, 3.0),
            (2, 2, 1.0),
        ],
        48.0,
    );
    let n = w.saturating_mul(h).min(dark.len());
    match dither {
        Dither::Grayscale => dark.iter().take(n).map(|d| d.clamp(0.0, 1.0)).collect(),
        Dither::Threshold => dark.iter().take(n).map(|d| if *d >= 0.5 { 1.0 } else { 0.0 }).collect(),
        Dither::Ordered => dark
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, d)| {
                let (x, y) = (i % w.max(1), i / w.max(1));
                let t = BAYER.get(y % 4).and_then(|r| r.get(x % 4)).copied().unwrap_or(0.0);
                if *d > (t + 0.5) / 16.0 { 1.0 } else { 0.0 }
            })
            .collect(),
        Dither::FloydSteinberg | Dither::Jarvis => {
            let (kernel, sum) = if dither == Dither::Jarvis { JJN } else { FS };
            let mut v: Vec<f32> = dark.iter().take(n).copied().collect();
            let mut out = vec![0.0; n];
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let Some(&old) = v.get(i) else { continue };
                    let new = if old >= 0.5 { 1.0 } else { 0.0 };
                    if let Some(o) = out.get_mut(i) {
                        *o = new;
                    }
                    let err = old - new;
                    for &(dx, dy, wt) in kernel {
                        let (nx, ny) = (x as isize + dx, y as isize + dy);
                        if nx >= 0
                            && (nx as usize) < w
                            && (ny as usize) < h
                            && let Some(t) = v.get_mut(ny as usize * w + nx as usize)
                        {
                            *t += err * wt / sum;
                        }
                    }
                }
            }
            out
        }
    }
}

/// The power (percent) of burn `level` between `min` and `max` (`None`: no burn).
pub(crate) fn power(level: f32, min: f64, max: f64) -> Option<f64> {
    (level > 1e-3).then(|| min + (max - min) * f64::from(level.clamp(0.0, 1.0)))
}

/// The samples (columns, rows) an image of `size_mm` (width, height along its own axes) gets at
/// `interval` mm: an error when there would be too many.
pub(crate) fn grid(size_mm: (f64, f64), interval: f64) -> Result<(u32, u32), String> {
    let n = |len: f64| (len / interval).round().max(1.0);
    let (c, r) = (n(size_mm.0), n(size_mm.1));
    if !(c.is_finite() && r.is_finite()) || c > MAX_SIDE as f64 || r > MAX_SIDE as f64 || c * r > MAX_SAMPLES as f64 {
        return Err(format!(
            "an image is too large to engrave at a {interval} mm interval (more than {MAX_SAMPLES} samples or {MAX_SIDE} along a side): use a larger interval"
        ));
    }
    Ok((c as u32, r as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A left-to-right grey ramp, `w` × `h`.
    fn ramp(w: usize, h: usize) -> Vec<f32> {
        (0..w * h).map(|i| (i % w) as f32 / (w - 1) as f32).collect()
    }

    #[test]
    fn dithering_is_deterministic_and_keeps_the_average() {
        let (w, h) = (64, 32);
        let dark = ramp(w, h);
        for d in [Dither::Threshold, Dither::Ordered, Dither::FloydSteinberg, Dither::Jarvis] {
            let a = levels(&dark, w, h, d);
            assert_eq!(a, levels(&dark, w, h, d), "{d:?} repeats exactly");
            assert!(a.iter().all(|v| *v == 0.0 || *v == 1.0), "{d:?}: on or off");
            let mean = a.iter().sum::<f32>() / a.len() as f32;
            assert!((mean - 0.5).abs() < 0.05, "{d:?}: {mean}");
            // Darker columns burn more.
            let col = |x: usize| (0..h).map(|y| a[y * w + x]).sum::<f32>();
            assert!(col(w - 4) > col(3), "{d:?}");
        }
        // Error diffusion and ordered spread a mid grey; threshold doesn't.
        let flat = vec![0.4; 16 * 16];
        assert!(levels(&flat, 16, 16, Dither::Threshold).iter().all(|v| *v == 0.0));
        for d in [Dither::Ordered, Dither::FloydSteinberg, Dither::Jarvis] {
            let on = levels(&flat, 16, 16, d).iter().filter(|v| **v == 1.0).count() as f32 / 256.0;
            assert!((on - 0.4).abs() < 0.07, "{d:?}: {on}");
        }
        // Grayscale keeps the levels.
        assert_eq!(levels(&[0.0, 0.25, 1.0], 3, 1, Dither::Grayscale), vec![0.0, 0.25, 1.0]);
    }

    #[test]
    fn darkness_and_power_mapping() {
        let px = [[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 0], [128, 128, 128, 255]];
        let d = darkness(&px, false);
        assert_eq!((d[0], d[1], d[2]), (1.0, 0.0, 0.0));
        assert!((d[3] - 0.498).abs() < 0.01);
        let n = darkness(&px, true);
        assert_eq!((n[0], n[1], n[2]), (0.0, 1.0, 0.0), "negative: white burns, transparent never");
        assert_eq!(power(0.0, 10.0, 60.0), None);
        assert_eq!(power(1.0, 10.0, 60.0), Some(60.0));
        assert_eq!(power(0.5, 10.0, 60.0), Some(35.0));
        assert_eq!(power(2.0, 10.0, 60.0), Some(60.0));
    }

    #[test]
    fn sample_grid_is_capped() {
        assert_eq!(grid((10.0, 5.0), 0.1).unwrap(), (100, 50));
        assert_eq!(grid((0.01, 0.01), 0.1).unwrap(), (1, 1));
        assert!(grid((5000.0, 5000.0), 0.01).is_err());
        assert!(grid((1.0e9, 1.0), 1.0).is_err());
        assert!(grid((f64::INFINITY, 1.0), 1.0).is_err());
        // Mismatched sizes are clamped, never out of bounds.
        assert_eq!(levels(&[1.0; 3], 4, 4, Dither::Jarvis).len(), 3);
        assert!(levels(&[], 0, 0, Dither::FloydSteinberg).is_empty());
        assert_eq!(Dither::parse("floyd-steinberg"), Some(Dither::FloydSteinberg));
        assert_eq!(Dither::parse("Jarvis"), Some(Dither::Jarvis));
        assert_eq!(Dither::parse("halftone"), None);
    }
}
