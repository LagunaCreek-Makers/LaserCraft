//! How long a job takes: its moves planned the way a motion controller plans them, each with a
//! trapezoidal speed profile (accelerate, cruise, decelerate) and the speed it may keep through
//! each corner (junction deviation), with a look-ahead over the whole job. Rapids run at the
//! axes' maximum rate; dwells (G4) add their time. An estimate: the controller's real planner,
//! its buffer and the serial link make the real time somewhat longer.

/// The machine's motion limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// X and Y maximum rates (mm/min, `$110`, `$111`).
    pub max_rate: [f64; 2],
    /// X and Y acceleration (mm/s², `$120`, `$121`).
    pub acceleration: [f64; 2],
    /// Junction deviation (mm, `$11`): how far a corner may be rounded at speed.
    pub junction_deviation: f64,
}

impl Default for Motion {
    /// Typical hobby diode and CO₂ machine values, used when the machine's aren't known.
    fn default() -> Self {
        Self { max_rate: [6000.0, 6000.0], acceleration: [1000.0, 1000.0], junction_deviation: 0.01 }
    }
}

impl Motion {
    /// The limits `settings` give, the defaults for those not read.
    pub fn from_settings(settings: &super::Settings) -> Self {
        let d = Self::default();
        let ok = |v: Option<(f64, f64)>, fallback: [f64; 2]| {
            v.filter(|(a, b)| a.is_finite() && b.is_finite() && *a > 0.0 && *b > 0.0).map_or(fallback, |(a, b)| [a, b])
        };
        Self {
            max_rate: ok(settings.max_rate(), d.max_rate),
            acceleration: ok(settings.acceleration(), d.acceleration),
            junction_deviation: settings.get(11).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d.junction_deviation),
        }
    }
}

/// What a job takes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Estimate {
    pub seconds: f64,
    /// Distance moved with the laser on (S above 0) and off (mm).
    pub burn_mm: f64,
    pub travel_mm: f64,
    pub moves: usize,
}

/// One straight move, ready to plan.
struct Move {
    len: f64,
    /// Unit direction.
    dir: [f64; 2],
    /// Fastest speed (mm/s) and acceleration (mm/s²) along it.
    vmax: f64,
    accel: f64,
}

/// The speed and acceleration a move along `dir` may have: the slowest axis's, scaled by how much
/// of the move is along it.
fn limits(dir: [f64; 2], feed: f64, m: &Motion) -> (f64, f64) {
    let mut v = feed;
    let mut a = f64::INFINITY;
    for ((d, rate), acc) in dir.iter().zip(m.max_rate).zip(m.acceleration) {
        let c = d.abs();
        if c > 1e-9 {
            v = v.min(rate / 60.0 / c);
            a = a.min(acc / c);
        }
    }
    (v.max(1e-6), if a.is_finite() { a.max(1e-6) } else { 1e-6 })
}

/// The time for `len` mm entered at `u`, left at `w`, cruising at most at `v`, with acceleration `a`.
fn trapezoid(len: f64, u: f64, w: f64, v: f64, a: f64) -> f64 {
    let d_acc = (v * v - u * u).max(0.0) / (2.0 * a);
    let d_dec = (v * v - w * w).max(0.0) / (2.0 * a);
    if d_acc + d_dec <= len {
        (v - u).max(0.0) / a + (v - w).max(0.0) / a + (len - d_acc - d_dec) / v
    } else {
        // Never reaches cruise: peaks where the two ramps meet.
        let peak = ((2.0 * a * len + u * u + w * w) / 2.0).sqrt().min(v);
        (peak - u).max(0.0) / a + (peak - w).max(0.0) / a
    }
}

/// Plans `moves` (look-ahead over all of them, starting and ending at rest) and adds up their time.
fn plan(moves: &[Move], m: &Motion) -> f64 {
    let n = moves.len();
    if n == 0 {
        return 0.0;
    }
    // The fastest each move may be entered: limited by the corner with the move before it.
    let mut entry = vec![0.0; n];
    for i in 1..n {
        let (p, q) = (&moves[i - 1], &moves[i]);
        let cos = -(p.dir[0] * q.dir[0] + p.dir[1] * q.dir[1]);
        let v = if cos > 0.999_999 {
            0.0
        } else if cos < -0.999_999 {
            f64::INFINITY
        } else {
            let sin_half = ((1.0 - cos) / 2.0).sqrt();
            (q.accel.min(p.accel) * m.junction_deviation * sin_half / (1.0 - sin_half)).sqrt()
        };
        entry[i] = v.min(p.vmax).min(q.vmax);
    }
    // Backwards: each move must be able to slow to the next one's entry (the last ends at rest).
    let mut exit = 0.0;
    for i in (0..n).rev() {
        let mv = &moves[i];
        entry[i] = entry[i].min((exit * exit + 2.0 * mv.accel * mv.len).sqrt());
        exit = entry[i];
    }
    // Forwards: each move can only reach what it can accelerate to from its entry.
    let mut total = 0.0;
    for i in 0..n {
        let mv = &moves[i];
        let reach = (entry[i] * entry[i] + 2.0 * mv.accel * mv.len).sqrt();
        let next = if i + 1 < n { entry[i + 1].min(reach) } else { 0.0 };
        if i + 1 < n {
            entry[i + 1] = next;
        }
        total += trapezoid(mv.len, entry[i], next, mv.vmax, mv.accel);
    }
    total
}

/// The time `code` takes on a machine with `motion`.
pub fn estimate(code: &str, motion: &Motion) -> Estimate {
    let mut out = Estimate::default();
    let mut moves = vec![];
    let mut pos = [0.0f64; 2];
    let mut absolute = true;
    let mut rapid = true;
    let mut feed = 1000.0 / 60.0;
    let mut power = 0.0;
    let mut dwell = 0.0;
    for raw in code.lines() {
        let line = raw.split(';').next().unwrap_or("").to_ascii_uppercase();
        let mut target = pos;
        let mut moved = false;
        let mut g4 = false;
        for w in line.split_whitespace() {
            let Some((&letter, rest)) = w.as_bytes().split_first() else { continue };
            let Ok(v) = std::str::from_utf8(rest).unwrap_or("").parse::<f64>() else { continue };
            if !v.is_finite() {
                continue;
            }
            match letter {
                b'G' => match v as i64 {
                    0 => rapid = true,
                    1..=3 => rapid = false,
                    4 => g4 = true,
                    90 => absolute = true,
                    91 => absolute = false,
                    _ => {}
                },
                b'X' => {
                    target[0] = if absolute { v } else { target[0] + v };
                    moved = true;
                }
                b'Y' => {
                    target[1] = if absolute { v } else { target[1] + v };
                    moved = true;
                }
                b'F' if v > 0.0 => feed = v / 60.0,
                b'S' => power = v,
                b'P' if g4 => dwell += v.max(0.0),
                _ => {}
            }
        }
        if g4 {
            // A dwell stops the planner: plan what came before, then wait.
            out.seconds += plan(&moves, motion);
            moves.clear();
            continue;
        }
        if !moved {
            continue;
        }
        let d = [target[0] - pos[0], target[1] - pos[1]];
        let len = d[0].hypot(d[1]);
        pos = target;
        if !(len > 1e-9 && len.is_finite()) {
            continue;
        }
        let dir = [d[0] / len, d[1] / len];
        let (vmax, accel) = limits(dir, if rapid { f64::INFINITY } else { feed }, motion);
        if !rapid && power > 0.0 {
            out.burn_mm += len;
        } else {
            out.travel_mm += len;
        }
        out.moves += 1;
        moves.push(Move { len, dir, vmax, accel });
    }
    out.seconds += plan(&moves, motion) + dwell;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn one_straight_move_is_a_trapezoid() {
        let m = Motion { max_rate: [6000.0, 6000.0], acceleration: [100.0, 100.0], junction_deviation: 0.01 };
        // 100 mm at 600 mm/min (10 mm/s), a = 100: ramps of 0.1 s and 0.5 mm each.
        let e = estimate("G1 X100 F600 S10", &m);
        assert!(close(e.seconds, 0.1 + 0.1 + 99.0 / 10.0, 1e-6), "{}", e.seconds);
        assert_eq!((e.burn_mm, e.travel_mm, e.moves), (100.0, 0.0, 1));
        // Too short to reach the feed: a triangle.
        let e = estimate("G1 X1 F6000 S10", &m);
        assert!(close(e.seconds, 2.0 * (1.0f64 / 100.0).sqrt(), 1e-6), "{}", e.seconds);
    }

    #[test]
    fn rapids_use_the_maximum_rate_and_dwells_add() {
        let m = Motion { max_rate: [3000.0, 3000.0], acceleration: [1e9, 1e9], junction_deviation: 0.01 };
        let e = estimate("G0 X50\nG4 P1.5", &m);
        assert!(close(e.seconds, 1.0 + 1.5, 1e-3), "{}", e.seconds);
        assert_eq!((e.travel_mm, e.burn_mm), (50.0, 0.0));
        // Diagonal: limited by the slower axis's share.
        let slow_y = Motion { max_rate: [6000.0, 600.0], ..m };
        let e = estimate("G0 X10 Y10", &slow_y);
        assert!(close(e.seconds, 10.0 / 10.0, 1e-3), "{}", e.seconds);
    }

    #[test]
    fn straight_runs_keep_speed_and_reversals_stop() {
        let m = Motion { max_rate: [6000.0, 6000.0], acceleration: [500.0, 500.0], junction_deviation: 0.01 };
        // A 100 mm line in 1 mm pieces takes about as long as one 100 mm move.
        let pieces: String = (1..=100).map(|i| format!("G1 X{i} F3000 S10\n")).collect();
        let whole = estimate("G1 X100 F3000 S10", &m).seconds;
        assert!(close(estimate(&pieces, &m).seconds, whole, 0.01), "{} vs {whole}", estimate(&pieces, &m).seconds);
        // Back and forth: each reversal stops, so it takes much longer than the same distance straight.
        let zigzag: String = (0..50).map(|i| format!("G1 X{} F3000 S10\n", if i % 2 == 0 { 2 } else { 0 })).collect();
        let straight = estimate("G1 X100 F3000 S10", &m).seconds;
        assert!(estimate(&zigzag, &m).seconds > straight * 2.0);
        // A square's corners slow it a little, not to rest-to-rest.
        let square = "G1 X50 F3000 S10\nG1 Y50\nG1 X0\nG1 Y0";
        let stop_go: f64 = (0..4).map(|_| estimate("G1 X50 F3000 S10", &m).seconds).sum();
        let s = estimate(square, &m).seconds;
        assert!(s < stop_go && s > 200.0 / 50.0, "{s} vs {stop_go}");
    }

    #[test]
    fn settings_give_the_motion_limits() {
        let mut s = super::super::Settings::default();
        assert_eq!(Motion::from_settings(&s), Motion::default());
        for (id, v) in [(110, 3000.0), (111, 2000.0), (120, 200.0), (121, 150.0), (11, 0.02)] {
            s.set(id, v);
        }
        assert_eq!(Motion::from_settings(&s), Motion { max_rate: [3000.0, 2000.0], acceleration: [200.0, 150.0], junction_deviation: 0.02 });
        s.set(120, -1.0);
        assert_eq!(Motion::from_settings(&s).acceleration, Motion::default().acceleration);
        // Junk lines don't break it.
        let e = estimate("G1 Xnan Y1\nG1 X1e400\nM5\n\nG1 X", &Motion::default());
        assert!(e.seconds.is_finite());
    }
}
