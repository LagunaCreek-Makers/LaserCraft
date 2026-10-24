//! Offline G-code dry-run. Never communicates with hardware.
#[derive(Clone, Debug)]
pub struct Step {
    pub line: usize,
    pub code: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub laser_on: bool,
    pub travel: bool,
    pub seconds: f64,
}
#[derive(Default)]
pub struct Simulation {
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    pub duration_seconds: f64,
}

pub fn simulate(source: &str, max_s: f64) -> Simulation {
    let mut result = Simulation::default();
    let (mut x, mut y, mut z, mut feed, mut power) = (0.0, 0.0, 0.0, 1000.0, 0.0);
    let (mut absolute, mut metric, mut motion, mut laser) = (true, true, 0u8, false);
    for (idx, raw) in source.lines().enumerate() {
        let code = raw.split(';').next().unwrap_or("").split('(').next().unwrap_or("").trim().to_uppercase();
        if code.is_empty() {
            continue;
        }
        let mut words = Vec::new();
        let mut current = None;
        let mut number = String::new();
        for c in code.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_alphabetic() || c == ' ' {
                if let Some(k) = current.take() {
                    if let Ok(v) = number.parse::<f64>() {
                        words.push((k, v));
                    }
                }
                number.clear();
                if c != ' ' {
                    current = Some(c);
                }
            } else if current.is_some() {
                number.push(c);
            }
        }
        let mut coords = (None, None, None);
        let mut has_coord = false;
        for &(k, v) in &words {
            if k == 'G' {
                let g = v.round() as i32;
                match g {
                    0 => motion = 0,
                    1 => motion = 1,
                    2 => motion = 2,
                    3 => motion = 3,
                    20 => metric = false,
                    21 => metric = true,
                    90 => absolute = true,
                    91 => absolute = false,
                    _ => {}
                }
            }
        }
        for &(k, v) in &words {
            match k {
                'M' => match v.round() as i32 {
                    3 | 4 => laser = true,
                    5 => laser = false,
                    _ => {}
                },
                'F' => feed = v,
                'S' => power = v,
                'X' => {
                    coords.0 = Some(v);
                    has_coord = true
                }
                'Y' => {
                    coords.1 = Some(v);
                    has_coord = true
                }
                'Z' => {
                    coords.2 = Some(v);
                    has_coord = true
                }
                _ => {}
            }
        }
        if !has_coord {
            continue;
        }
        let unit = if metric { 1.0 } else { 25.4 };
        let dest = |old: f64, val: Option<f64>| val.map(|v| if absolute { v * unit } else { old + v * unit }).unwrap_or(old);
        let (nx, ny, nz) = (dest(x, coords.0), dest(y, coords.1), dest(z, coords.2));
        if !nx.is_finite() || !ny.is_finite() || !nz.is_finite() {
            result.warnings.push(format!("Line {}: nonfinite coordinates", idx + 1));
            continue;
        }
        if motion == 2 || motion == 3 {
            let i = words.iter().find(|(k, _)| *k == 'I').map(|(_, v)| v * unit);
            let j = words.iter().find(|(k, _)| *k == 'J').map(|(_, v)| v * unit);
            if let (Some(i), Some(j)) = (i, j) {
                let (cx, cy) = (x + i, y + j);
                let r = i.hypot(j);
                let end_r = (nx - cx).hypot(ny - cy);
                if r > 1e-9 && (end_r - r).abs() <= 0.05_f64.max(r * 0.001) {
                    let start = (y - cy).atan2(x - cx);
                    let finish = (ny - cy).atan2(nx - cx);
                    let full = std::f64::consts::TAU;
                    let mut sweep = if motion == 3 { (finish - start).rem_euclid(full) } else { (start - finish).rem_euclid(full) };
                    if sweep < 1e-10 {
                        sweep = full;
                    }
                    let segments = ((r * sweep / 0.25).ceil() as usize).clamp(1, 20000);
                    let mut px = x;
                    let mut py = y;
                    let mut pz = z;
                    for k in 1..=segments {
                        let f = k as f64 / segments as f64;
                        let theta = start + if motion == 3 { sweep * f } else { -sweep * f };
                        let (ax, ay, az) = if k == segments { (nx, ny, nz) } else { (cx + r * theta.cos(), cy + r * theta.sin(), z + (nz - z) * f) };
                        let distance = (ax - px).hypot(ay - py).hypot(az - pz);
                        let seconds = if feed > 0.0 { distance / (feed * unit) * 60.0 } else { 0.0 };
                        result.duration_seconds += seconds;
                        let active = laser && power > 0.0 && max_s > 0.0;
                        result.steps.push(Step {
                            line: idx + 1,
                            code: raw.to_string(),
                            x: ax,
                            y: ay,
                            z: az,
                            laser_on: active,
                            travel: !active,
                            seconds,
                        });
                        (px, py, pz) = (ax, ay, az);
                    }
                    (x, y, z) = (nx, ny, nz);
                    continue;
                }
            }
            result.warnings.push(format!("Line {}: unsupported or inconsistent arc (requires I/J center offsets); skipped", idx + 1));
            continue;
        }
        let dist = ((nx - x).powi(2) + (ny - y).powi(2) + (nz - z).powi(2)).sqrt();
        let speed = if motion == 0 { 3000.0 } else { feed * unit };
        let seconds = if speed > 0.0 { dist / speed * 60.0 } else { 0.0 };
        result.duration_seconds += seconds;
        let active = motion == 1 && laser && power > 0.0 && max_s > 0.0;
        result.steps.push(Step { line: idx + 1, code: raw.to_string(), x: nx, y: ny, z: nz, laser_on: active, travel: !active, seconds });
        (x, y, z) = (nx, ny, nz);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modal_absolute_and_relative() {
        let s = simulate("G21 G90\nG0 X10 Y5\nM4 S500\nG1 X20 F600\nG91\nG1 X-5\nM5", 1000.0);
        assert_eq!(s.steps.len(), 3);
        assert_eq!(s.steps[2].x, 15.0);
        assert!(s.steps[1].laser_on);
        assert!(!s.steps[0].laser_on);
    }
    #[test]
    fn arc_is_tessellated() {
        let s = simulate("G21 G90 M4 S500\nG0 X10 Y0\nG3 X0 Y10 I-10 J0 F600", 1000.0);
        assert!(s.steps.len() > 10);
        assert!((s.steps.last().unwrap().x).abs() < 1e-8);
        assert!((s.steps.last().unwrap().y - 10.0).abs() < 1e-8);
    }
    #[test]
    fn invalid_arc_is_flagged() {
        assert!(!simulate("G2 X10 Y10 I5 J0", 1000.0).warnings.is_empty());
    }
}

/// Interpolated virtual head position; does not communicate with hardware.
#[derive(Clone, Copy, Debug)]
pub struct HeadPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub laser_on: bool,
    pub step_index: usize,
}

pub fn step_start_time(sim: &Simulation, index: usize) -> f64 {
    sim.steps.iter().take(index.min(sim.steps.len())).map(|s| s.seconds.max(0.0)).sum()
}
pub fn step_end_time(sim: &Simulation, index: usize) -> f64 {
    step_start_time(sim, index.saturating_add(1))
}

pub fn position_at(sim: &Simulation, time: f64) -> HeadPosition {
    let mut previous = (0.0, 0.0, 0.0);
    let mut elapsed = 0.0;
    for (index, step) in sim.steps.iter().enumerate() {
        let end = elapsed + step.seconds.max(0.0);
        if time < end || (index + 1 == sim.steps.len()) {
            let fraction = if step.seconds > 0.0 { ((time - elapsed) / step.seconds).clamp(0.0, 1.0) } else { 1.0 };
            let mix = |a: f64, b: f64| a + (b - a) * fraction;
            return HeadPosition {
                x: mix(previous.0, step.x),
                y: mix(previous.1, step.y),
                z: mix(previous.2, step.z),
                laser_on: step.laser_on && fraction < 1.0,
                step_index: index,
            };
        }
        elapsed = end;
        previous = (step.x, step.y, step.z);
    }
    HeadPosition { x: 0.0, y: 0.0, z: 0.0, laser_on: false, step_index: 0 }
}

#[cfg(test)]
mod phase16_tests {
    use super::*;
    #[test]
    fn interpolates_halfway() {
        let sim = simulate("G21 G90\nG1 X10 F600\nG1 Y10 F600", 1000.0);
        let p = position_at(&sim, 0.5);
        assert!((p.x - 5.0).abs() < 1e-8);
        assert!(p.y.abs() < 1e-8);
        assert_eq!(p.step_index, 0);
        let q = position_at(&sim, 1.5);
        assert!((q.x - 10.0).abs() < 1e-8);
        assert!((q.y - 5.0).abs() < 1e-8);
    }
    #[test]
    fn clamps_timeline() {
        let sim = simulate("G1 X10 F600", 1000.0);
        assert_eq!(position_at(&sim, -100.0).x, 0.0);
        assert_eq!(position_at(&sim, 100.0).x, 10.0);
    }
}

/// Cumulative *relative* exposure map for preview only; not a material physics model.
/// Each laser-on segment contributes length-weighted exposure to intersected cells.
#[derive(Clone, Debug)]
pub struct ExposureGrid {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
    pub peak: f32,
}
pub fn exposure_grid(sim: &Simulation, bed_w: f64, bed_h: f64, width: usize, height: usize, until: f64) -> ExposureGrid {
    let width = width.clamp(1, 1024);
    let height = height.clamp(1, 1024);
    let mut values = vec![0.0_f32; width * height];
    if !(bed_w > 0.0 && bed_h > 0.0) {
        return ExposureGrid { width, height, values, peak: 0.0 };
    }
    let mut prev = (0.0, 0.0);
    let mut elapsed = 0.0;
    for step in &sim.steps {
        let portion = if step.seconds > 0.0 { ((until - elapsed) / step.seconds).clamp(0.0, 1.0) } else { if until >= elapsed { 1.0 } else { 0.0 } };
        if portion <= 0.0 {
            break;
        }
        let end = (prev.0 + (step.x - prev.0) * portion, prev.1 + (step.y - prev.1) * portion);
        if step.laser_on {
            let dx = (end.0 - prev.0) / bed_w * width as f64;
            let dy = (end.1 - prev.1) / bed_h * height as f64;
            let samples = (dx.hypot(dy).ceil() as usize).clamp(1, 100000);
            let length = (end.0 - prev.0).hypot(end.1 - prev.1);
            for k in 0..=samples {
                let f = k as f64 / samples as f64;
                let x = prev.0 + (end.0 - prev.0) * f;
                let y = prev.1 + (end.1 - prev.1) * f;
                if x >= 0.0 && y >= 0.0 && x < bed_w && y < bed_h {
                    let col = (x / bed_w * width as f64) as usize;
                    let row = (y / bed_h * height as f64) as usize;
                    values[row * width + col] += (length / samples as f64) as f32;
                }
            }
        }
        elapsed += step.seconds.max(0.0);
        prev = (step.x, step.y);
        if portion < 1.0 {
            break;
        }
    }
    let peak = values.iter().copied().fold(0.0_f32, f32::max);
    ExposureGrid { width, height, values, peak }
}
#[cfg(test)]
mod phase17_tests {
    use super::*;
    #[test]
    fn exposure_only_for_laser_on() {
        let sim = simulate("G0 X1 Y1\nM4 S500\nG1 X9 Y1 F600\nM5\nG0 X9 Y9", 1000.0);
        let grid = exposure_grid(&sim, 10.0, 10.0, 20, 20, 100.0);
        assert!(grid.peak > 0.0);
        assert_eq!(grid.values[19 * 20 + 19], 0.0);
    }
    #[test]
    fn exposure_increases_with_time() {
        let sim = simulate("M4 S500\nG1 X10 F600", 1000.0);
        let a = exposure_grid(&sim, 20.0, 20.0, 20, 20, 0.25);
        let b = exposure_grid(&sim, 20.0, 20.0, 20, 20, 1.0);
        assert!(b.values.iter().sum::<f32>() > a.values.iter().sum::<f32>());
    }
}

/// Offline trapezoidal speed estimate. Each move assumes a stop at both ends;
/// actual controller look-ahead, junction deviation and firmware differ.
#[derive(Clone, Copy, Debug)]
pub struct MotionEstimate {
    pub acceleration_mm_s2: f64,
    pub rapid_mm_min: f64,
}
impl Default for MotionEstimate {
    fn default() -> Self {
        Self { acceleration_mm_s2: 500.0, rapid_mm_min: 3000.0 }
    }
}

pub fn estimated_motion_time(sim: &Simulation, settings: MotionEstimate) -> f64 {
    let acceleration = settings.acceleration_mm_s2.max(0.001);
    let mut previous = (0.0, 0.0, 0.0);
    let mut total = 0.0;
    for step in &sim.steps {
        let distance = ((step.x - previous.0).powi(2) + (step.y - previous.1).powi(2) + (step.z - previous.2).powi(2)).sqrt();
        let nominal = if step.seconds > 0.0 { distance / step.seconds } else { 0.0 };
        let vmax = if step.travel { settings.rapid_mm_min.max(1.0) / 60.0 } else { nominal };
        if distance > 0.0 && vmax > 0.0 {
            let ramp = vmax * vmax / acceleration;
            total += if distance >= ramp { 2.0 * vmax / acceleration + (distance - ramp) / vmax } else { 2.0 * (distance / acceleration).sqrt() };
        }
        previous = (step.x, step.y, step.z);
    }
    total
}

/// Simple 8-bit raster image to bidirectional GRBL scanlines.
/// Pixels are row-major, with 0 black and 255 white. Offline output only.
pub fn raster_to_gcode(
    pixels: &[u8],
    width: usize,
    height: usize,
    pixel_mm: f64,
    feed_mm_min: f64,
    max_s: u32,
    threshold: Option<u8>,
) -> Result<String, String> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 || pixels.len() != width * height {
        return Err("Invalid raster dimensions".into());
    }
    if !pixel_mm.is_finite() || pixel_mm <= 0.0 || !feed_mm_min.is_finite() || feed_mm_min <= 0.0 || max_s == 0 {
        return Err("Invalid raster settings".into());
    }
    let mut out = format!("G21\nG90\nM4 S0\nG1 F{feed_mm_min:.2}\n");
    for row in 0..height {
        let reverse = row % 2 == 1;
        let y = row as f64 * pixel_mm;
        let first = if reverse { width - 1 } else { 0 };
        out.push_str(&format!("M5\nG0 X{:.4} Y{y:.4}\nM4 S0\n", first as f64 * pixel_mm));
        for n in 0..width {
            let col = if reverse { width - 1 - n } else { n };
            let gray = pixels[row * width + col];
            let darkness = if let Some(cut) = threshold { if gray < cut { 1.0 } else { 0.0 } } else { (255 - gray) as f64 / 255.0 };
            let power = (darkness * max_s as f64).round() as u32;
            out.push_str(&format!("G1 X{:.4} Y{y:.4} S{power}\n", (col as f64 + if reverse { -0.5 } else { 0.5 }) * pixel_mm));
        }
    }
    out.push_str("M5\n");
    Ok(out)
}

#[cfg(test)]
mod phase18_tests {
    use super::*;
    #[test]
    fn raster_respects_black_white_and_bidirectional() {
        let code = raster_to_gcode(&[0, 255, 255, 0], 2, 2, 0.1, 1200.0, 1000, None).unwrap();
        assert!(code.contains("S1000"));
        assert!(code.contains("S0"));
        assert!(code.contains("G0 X0.1000 Y0.1000"));
    }
    #[test]
    fn motion_estimate_is_longer_with_acceleration() {
        let sim = simulate("G21 G90\nG1 X100 F6000", 1000.0);
        let estimate = estimated_motion_time(&sim, MotionEstimate::default());
        assert!(estimate > sim.duration_seconds);
    }
    #[test]
    fn bad_raster_rejected() {
        assert!(raster_to_gcode(&[], 1, 1, 0.1, 100.0, 1000, None).is_err());
    }
}
