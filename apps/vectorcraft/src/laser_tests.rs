//! Offline laser calibration pattern generator. Does not communicate with hardware.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestKind {
    PowerSpeed,
    Interval,
    Passes,
    Grayscale,
    MinimumPower,
    Kerf,
    GrayscaleGradient,
    Dither,
    Focus,
    ScanDirection,
}
impl TestKind {
    pub const ALL: [Self; 10] = [
        Self::PowerSpeed,
        Self::Interval,
        Self::Passes,
        Self::Grayscale,
        Self::MinimumPower,
        Self::Kerf,
        Self::GrayscaleGradient,
        Self::Dither,
        Self::Focus,
        Self::ScanDirection,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerSpeed => "Power / speed matrix",
            Self::Interval => "Line interval / DPI",
            Self::Passes => "Pass count",
            Self::Grayscale => "Grayscale power ramp",
            Self::MinimumPower => "Minimum visible power",
            Self::Kerf => "Kerf / dimensional squares",
            Self::GrayscaleGradient => "Continuous grayscale gradient",
            Self::Dither => "Binary dither grayscale",
            Self::Focus => "Focus line-width ladder",
            Self::ScanDirection => "Bidirectional scan comparison",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSettings {
    pub kind: TestKind,
    pub rows: u32,
    pub columns: u32,
    pub cell_mm: f64,
    pub gap_mm: f64,
    pub origin_x: f64,
    pub origin_y: f64,
    pub speed_min: f64,
    pub speed_max: f64,
    pub power_min: f64,
    pub power_max: f64,
    pub interval_min: f64,
    pub interval_max: f64,
    pub passes_min: u32,
    pub passes_max: u32,
    #[serde(default = "default_gamma")]
    pub gamma: f64,
    #[serde(default = "default_steps")]
    pub gray_steps: u32,
    pub max_s: u32,
    pub bed_width: f64,
    pub bed_height: f64,
}
fn default_gamma() -> f64 {
    1.0
}
fn default_steps() -> u32 {
    16
}
impl Default for TestSettings {
    fn default() -> Self {
        Self {
            kind: TestKind::PowerSpeed,
            rows: 4,
            columns: 5,
            cell_mm: 12.0,
            gap_mm: 4.0,
            origin_x: 10.0,
            origin_y: 10.0,
            speed_min: 600.0,
            speed_max: 2400.0,
            power_min: 5.0,
            power_max: 40.0,
            interval_min: 0.08,
            interval_max: 0.3,
            passes_min: 1,
            passes_max: 5,
            gamma: 1.0,
            gray_steps: 16,
            max_s: 1000,
            bed_width: 300.0,
            bed_height: 200.0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub machine: String,
    pub material: String,
    pub settings: TestSettings,
    pub observations: String,
    pub preferred_speed: f64,
    pub preferred_power: f64,
    pub preferred_interval: f64,
    pub preferred_passes: u32,
}
fn interpolate(a: f64, b: f64, i: u32, n: u32) -> f64 {
    if n <= 1 { a } else { a + (b - a) * i as f64 / (n - 1) as f64 }
}
fn validate(s: &TestSettings) -> Result<(), String> {
    if s.rows == 0 || s.columns == 0 || s.rows > 20 || s.columns > 20 {
        return Err("Grid must contain 1–20 rows and columns".into());
    }
    let vals = [
        s.cell_mm,
        s.gap_mm,
        s.origin_x,
        s.origin_y,
        s.speed_min,
        s.speed_max,
        s.power_min,
        s.power_max,
        s.interval_min,
        s.interval_max,
        s.bed_width,
        s.bed_height,
        s.gamma,
    ];
    if vals.iter().any(|v| !v.is_finite()) {
        return Err("Non-finite parameter".into());
    }
    if s.cell_mm < 2.0
        || s.gap_mm < 0.0
        || s.speed_min <= 0.0
        || s.speed_max < s.speed_min
        || s.power_min < 0.0
        || s.power_max > 100.0
        || s.power_max < s.power_min
        || s.interval_min <= 0.0
        || s.interval_max < s.interval_min
        || s.passes_min == 0
        || s.passes_max < s.passes_min
        || s.passes_max > 50
        || s.max_s == 0
        || s.gamma <= 0.0
        || s.gamma > 5.0
        || s.gray_steps < 2
        || s.gray_steps > 256
    {
        return Err("Invalid test range".into());
    }
    if s.origin_x < 0.0
        || s.origin_y < 0.0
        || s.origin_x + (s.columns as f64) * s.cell_mm + (s.columns - 1) as f64 * s.gap_mm > s.bed_width
        || s.origin_y + (s.rows as f64) * s.cell_mm + (s.rows - 1) as f64 * s.gap_mm > s.bed_height
    {
        return Err("Test pattern exceeds machine bed".into());
    }
    Ok(())
}
fn rectangle(code: &mut String, x: f64, y: f64, size: f64, feed: f64, power: f64, max_s: u32) {
    let p = ((power / 100.0) * max_s as f64).round() as u32;
    code.push_str(&format!(
        "M5\nG0 X{x:.3} Y{y:.3}\nM4 S{p}\nG1 X{:.3} Y{y:.3} F{feed:.1}\nG1 X{:.3} Y{:.3}\nG1 X{x:.3} Y{:.3}\nG1 X{x:.3} Y{y:.3}\nM5\n",
        x + size,
        x + size,
        y + size,
        y + size
    ));
}
#[allow(clippy::too_many_arguments)]
fn scanline(code: &mut String, x: f64, y: f64, w: f64, speed: f64, power: f64, max_s: u32, reverse: bool) {
    let (a, b) = if reverse { (x + w, x) } else { (x, x + w) };
    let p = ((power / 100.0) * max_s as f64).round() as u32;
    code.push_str(&format!("M5\nG0 X{a:.3} Y{y:.3}\nM4 S{p}\nG1 X{b:.3} Y{y:.3} F{speed:.1}\nM5\n"));
}
#[allow(clippy::too_many_arguments)]
fn raster(code: &mut String, x: f64, y: f64, size: f64, pitch: f64, speed: f64, power: f64, max_s: u32, bidirectional: bool) {
    let lines = (size / pitch).floor() as usize;
    for n in 0..=lines {
        let yy = (y + n as f64 * pitch).min(y + size);
        scanline(code, x, yy, size, speed, power, max_s, bidirectional && n % 2 == 1);
    }
}
#[allow(clippy::too_many_arguments)]
fn gradient(
    code: &mut String,
    x: f64,
    y: f64,
    size: f64,
    pitch: f64,
    speed: f64,
    low: f64,
    high: f64,
    max_s: u32,
    gamma: f64,
    steps: u32,
    dither: bool,
) {
    let lines = (size / pitch).floor() as usize;
    for n in 0..=lines {
        let yy = (y + n as f64 * pitch).min(y + size);
        code.push_str(&format!("M5\nG0 X{x:.3} Y{yy:.3}\n"));
        for k in 0..steps {
            let fraction = (k as f64 + 0.5) / steps as f64;
            let gray = fraction.powf(gamma);
            let power = if dither {
                if ((k * 37 + n as u32 * 17) % steps) as f64 / (steps as f64) < gray { high } else { low }
            } else {
                low + (high - low) * gray
            };
            let pwm = ((power / 100.0) * max_s as f64).round() as u32;
            let endpoint = x + size * (k + 1) as f64 / steps as f64;
            code.push_str(&format!("M4 S{pwm}\nG1 X{endpoint:.3} Y{yy:.3} F{speed:.1}\n"));
        }
        code.push_str("M5\n");
    }
}
pub fn generate(s: &TestSettings) -> Result<String, String> {
    validate(s)?;
    let mut out = format!(
        "; LaserCraft OFFLINE {} calibration - inspect before running\n; M4 dynamic laser mode; compatible GRBL laser configuration required\n; gamma={:.3} gray_steps={}\nG21\nG90\nM5\n",
        s.kind.label(),
        s.gamma,
        s.gray_steps
    );
    for row in 0..s.rows {
        for col in 0..s.columns {
            let x = s.origin_x + col as f64 * (s.cell_mm + s.gap_mm);
            let y = s.origin_y + row as f64 * (s.cell_mm + s.gap_mm);
            let speed = if s.kind == TestKind::PowerSpeed || s.kind == TestKind::ScanDirection {
                interpolate(s.speed_min, s.speed_max, row, s.rows)
            } else {
                s.speed_min
            };
            let power = match s.kind {
                TestKind::PowerSpeed | TestKind::Grayscale | TestKind::MinimumPower => interpolate(s.power_min, s.power_max, col, s.columns),
                _ => s.power_max,
            };
            let interval = if s.kind == TestKind::Interval { interpolate(s.interval_min, s.interval_max, col, s.columns) } else { s.interval_min };
            let passes =
                if s.kind == TestKind::Passes { interpolate(s.passes_min as f64, s.passes_max as f64, col, s.columns).round() as u32 } else { 1 };
            let pitch = interval;
            if (s.cell_mm / pitch).ceil() > 2000.0 {
                return Err("Too many scanlines; increase interval".into());
            }
            out.push_str(&format!("; row={} col={} speed={speed:.1} power={power:.2}% interval={interval:.3} passes={passes}\n", row + 1, col + 1));
            for _ in 0..passes {
                match s.kind {
                    TestKind::PowerSpeed | TestKind::Passes | TestKind::Kerf => rectangle(&mut out, x, y, s.cell_mm, speed, power, s.max_s),
                    TestKind::MinimumPower => {
                        scanline(&mut out, x, y + s.cell_mm / 2.0, s.cell_mm, speed, power, s.max_s, false);
                    }
                    TestKind::Interval | TestKind::Grayscale => raster(&mut out, x, y, s.cell_mm, pitch, speed, power, s.max_s, true),
                    TestKind::GrayscaleGradient | TestKind::Dither => gradient(
                        &mut out,
                        x,
                        y,
                        s.cell_mm,
                        pitch,
                        speed,
                        s.power_min,
                        s.power_max,
                        s.max_s,
                        s.gamma,
                        s.gray_steps,
                        s.kind == TestKind::Dither,
                    ),
                    TestKind::Focus => {
                        let lines = 5;
                        for n in 0..lines {
                            scanline(&mut out, x, y + (n as f64 + 1.0) * s.cell_mm / (lines as f64 + 1.0), s.cell_mm, speed, power, s.max_s, false);
                        }
                    }
                    TestKind::ScanDirection => raster(&mut out, x, y, s.cell_mm, pitch, speed, power, s.max_s, col % 2 == 1),
                }
            }
        }
    }
    out.push_str("M5\n; End of test pattern\n");
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_patterns_generate() {
        for kind in TestKind::ALL {
            let s = TestSettings { kind, ..Default::default() };
            let g = generate(&s).unwrap();
            assert!(g.contains("G21"));
            assert!(g.ends_with("; End of test pattern\n"));
        }
    }
    #[test]
    fn rejects_out_of_bounds() {
        let s = TestSettings { origin_x: 299.0, ..Default::default() };
        assert!(generate(&s).is_err());
    }
    #[test]
    fn rejects_nonfinite() {
        let s = TestSettings { power_max: f64::NAN, ..Default::default() };
        assert!(generate(&s).is_err());
    }
    #[test]
    fn preserves_laser_off_at_end() {
        let s = TestSettings::default();
        let g = generate(&s).unwrap();
        assert!(g.contains("M5\n; End"));
    }
    #[test]
    fn gradient_varies_power() {
        let s = TestSettings { kind: TestKind::GrayscaleGradient, ..Default::default() };
        let g = generate(&s).unwrap();
        assert!(g.contains("M4 S"));
        let distinct: std::collections::HashSet<_> = g.lines().filter(|l| l.starts_with("M4 S")).collect();
        assert!(distinct.len() > 4);
    }
    #[test]
    fn dither_is_binary() {
        let s = TestSettings { kind: TestKind::Dither, ..Default::default() };
        let g = generate(&s).unwrap();
        let levels: std::collections::HashSet<_> = g.lines().filter(|l| l.starts_with("M4 S")).collect();
        assert_eq!(levels.len(), 2);
    }
    #[test]
    fn invalid_gamma() {
        let s = TestSettings { gamma: 0.0, ..Default::default() };
        assert!(generate(&s).is_err());
    }
}
