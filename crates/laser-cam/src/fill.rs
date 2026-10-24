//! Fill mode: the inside of closed shapes as parallel scan lines.
//!
//! The shapes are turned so that the scan lines run along x, then swept from bottom to top: at
//! each line's height the edges crossing it give, per shape, the stretches its fill rule (even-odd
//! or nonzero) counts as inside, so holes stay empty. Overlapping shapes are merged, so no spot is
//! burnt twice by one scan line. The stretches are turned back to the scan angle.

use vectorcraft_geom::{Affine, FillRule, Point};

/// Most scan lines one fill may have (a cap on the work a tiny interval on a big shape asks for).
pub(crate) const MAX_LINES: usize = 2_000_000;

/// A filled shape: its outlines (each closing implicitly) in bed millimetres, and its rule.
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    pub polys: Vec<Vec<Point>>,
    pub rule: FillRule,
}

/// One edge of a shape in the scan frame, `y0 < y1`.
#[derive(Clone, Copy)]
struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    /// +1 going up, -1 going down (for the nonzero rule).
    dir: i32,
    shape: usize,
}

impl Edge {
    fn x_at(&self, y: f64) -> f64 {
        self.x0 + (y - self.y0) * (self.x1 - self.x0) / (self.y1 - self.y0)
    }
}

/// The scan segments (start, end) filling `shapes` with lines `interval` apart at `angle_deg`,
/// in cutting order: line by line from one side to the other, each line's stretches along it,
/// every other line run back when `bidirectional`.
pub(crate) fn scan(shapes: &[Shape], interval: f64, angle_deg: f64, bidirectional: bool) -> Result<Vec<(Point, Point)>, String> {
    if !(interval.is_finite() && interval > 0.0 && angle_deg.is_finite()) {
        return Err("the fill interval and angle must be finite, the interval more than 0".into());
    }
    let turn = Affine::rotate(angle_deg.to_radians());
    let to_scan = turn.inverse();
    let mut edges = vec![];
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (si, shape) in shapes.iter().enumerate() {
        for poly in &shape.polys {
            let pts: Vec<Point> = poly.iter().map(|p| to_scan * *p).collect();
            let Some(&last) = pts.last() else { continue };
            let mut prev = last;
            for &p in &pts {
                if (p.y - prev.y).abs() > 1e-12 {
                    let (a, b, dir) = if p.y > prev.y { (prev, p, 1) } else { (p, prev, -1) };
                    edges.push(Edge { x0: a.x, y0: a.y, x1: b.x, y1: b.y, dir, shape: si });
                    lo = lo.min(a.y);
                    hi = hi.max(b.y);
                }
                prev = p;
            }
        }
    }
    if edges.is_empty() || !(lo.is_finite() && hi.is_finite()) {
        return Ok(vec![]);
    }
    let lines = ((hi - lo) / interval).ceil();
    if !(lines.is_finite() && lines <= MAX_LINES as f64) {
        return Err(format!("the fill needs more than {MAX_LINES} scan lines: use a larger interval"));
    }
    let lines = lines as usize;
    edges.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut next = 0;
    let mut active: Vec<Edge> = vec![];
    let mut crossings: Vec<(usize, f64, i32)> = vec![];
    let mut spans: Vec<(f64, f64)> = vec![];
    let mut out = vec![];
    let mut row = 0usize;
    for k in 0..lines {
        let y = lo + interval * (k as f64 + 0.5);
        if y >= hi {
            break;
        }
        while let Some(e) = edges.get(next).filter(|e| e.y0 <= y) {
            active.push(*e);
            next += 1;
        }
        // Each edge covers [y0, y1): a vertex shared by two edges is crossed once.
        active.retain(|e| e.y1 > y);
        crossings.clear();
        crossings.extend(active.iter().filter(|e| e.y0 <= y).map(|e| (e.shape, e.x_at(y), e.dir)));
        crossings.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        spans.clear();
        let mut i = 0;
        while i < crossings.len() {
            let shape = crossings.get(i).map_or(0, |c| c.0);
            let rule = shapes.get(shape).map_or(FillRule::NonZero, |s| s.rule);
            let mut winding = 0i32;
            let mut start = 0.0;
            while let Some(&(_, x, dir)) = crossings.get(i).filter(|c| c.0 == shape) {
                let was = inside(winding, rule);
                winding += dir;
                let now = inside(winding, rule);
                if !was && now {
                    start = x;
                } else if was && !now && x - start > 1e-9 {
                    spans.push((start, x));
                }
                i += 1;
            }
        }
        // Overlapping shapes: one stretch where they overlap.
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = vec![];
        for &(a, b) in spans.iter() {
            match merged.last_mut() {
                Some(m) if a <= m.1 => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        if merged.is_empty() {
            continue;
        }
        let back = bidirectional && row % 2 == 1;
        row += 1;
        let mut segs: Vec<(Point, Point)> = merged.iter().map(|&(a, b)| (turn * Point::new(a, y), turn * Point::new(b, y))).collect();
        if back {
            segs.reverse();
            for s in &mut segs {
                *s = (s.1, s.0);
            }
        }
        out.extend(segs);
    }
    Ok(out)
}

fn inside(winding: i32, rule: FillRule) -> bool {
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64, ccw: bool) -> Vec<Point> {
        let mut v = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        if !ccw {
            v.reverse();
        }
        v
    }

    fn total(segs: &[(Point, Point)]) -> f64 {
        segs.iter().map(|(a, b)| a.distance(*b)).sum()
    }

    #[test]
    fn a_square_is_filled_with_lines_an_interval_apart() {
        let s = [Shape { polys: vec![rect(0.0, 0.0, 10.0, 10.0, true)], rule: FillRule::NonZero }];
        let segs = scan(&s, 1.0, 0.0, false).unwrap();
        assert_eq!(segs.len(), 10);
        for (i, (a, b)) in segs.iter().enumerate() {
            assert!((a.y - (i as f64 + 0.5)).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
            assert!((a.x - 0.0).abs() < 1e-9 && (b.x - 10.0).abs() < 1e-9, "left to right: {a:?} {b:?}");
        }
        // Bidirectional: every other line runs back.
        let segs = scan(&s, 1.0, 0.0, true).unwrap();
        assert!(segs[0].0.x < segs[0].1.x && segs[1].0.x > segs[1].1.x && segs[2].0.x < segs[2].1.x);
        // Within the shape's bounds.
        assert!(segs.iter().all(|(a, b)| [a, b].iter().all(|p| (0.0..=10.0).contains(&p.x) && (0.0..=10.0).contains(&p.y))));
    }

    #[test]
    fn holes_stay_empty_under_either_rule() {
        // Even-odd: a hole drawn the same way round. Nonzero: a hole drawn the other way round.
        for (rule, hole_ccw) in [(FillRule::EvenOdd, true), (FillRule::NonZero, false)] {
            let s = [Shape { polys: vec![rect(0.0, 0.0, 10.0, 10.0, true), rect(3.0, 3.0, 7.0, 7.0, hole_ccw)], rule }];
            let segs = scan(&s, 1.0, 0.0, false).unwrap();
            for (a, b) in &segs {
                let mid_hole = a.y > 3.0 && a.y < 7.0;
                // No stretch burns across the hole's inside.
                assert!(!(mid_hole && a.x < 4.0 && b.x > 6.0), "{rule:?}: {a:?} → {b:?}");
            }
            // 10 lines × 10 mm, minus 4 lines × 4 mm of hole.
            assert!((total(&segs) - (100.0 - 16.0)).abs() < 1e-6, "{rule:?}: {}", total(&segs));
        }
        // Nonzero with the hole the same way round: no hole.
        let s = [Shape { polys: vec![rect(0.0, 0.0, 10.0, 10.0, true), rect(3.0, 3.0, 7.0, 7.0, true)], rule: FillRule::NonZero }];
        assert!((total(&scan(&s, 1.0, 0.0, false).unwrap()) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn overlapping_shapes_are_burnt_once() {
        let s = [
            Shape { polys: vec![rect(0.0, 0.0, 10.0, 10.0, true)], rule: FillRule::NonZero },
            Shape { polys: vec![rect(5.0, 0.0, 15.0, 10.0, true)], rule: FillRule::EvenOdd },
        ];
        let segs = scan(&s, 1.0, 0.0, false).unwrap();
        assert_eq!(segs.len(), 10);
        assert!((total(&segs) - 150.0).abs() < 1e-6);
    }

    #[test]
    fn angle_turns_the_lines() {
        let s = [Shape { polys: vec![rect(0.0, 0.0, 10.0, 4.0, true)], rule: FillRule::NonZero }];
        let segs = scan(&s, 1.0, 90.0, false).unwrap();
        // Vertical lines, one per mm across the 10 mm width, each 4 mm long.
        assert_eq!(segs.len(), 10);
        for (a, b) in &segs {
            assert!((a.x - b.x).abs() < 1e-9 && ((a.y - b.y).abs() - 4.0).abs() < 1e-9, "{a:?} {b:?}");
        }
        let mut xs: Vec<f64> = segs.iter().map(|s| s.0.x).collect();
        xs.sort_by(f64::total_cmp);
        for w in xs.windows(2) {
            assert!((w[1] - w[0] - 1.0).abs() < 1e-9);
        }
        // 45°: same area burnt, within the bounds.
        let segs = scan(&s, 0.1, 45.0, true).unwrap();
        assert!((total(&segs) * 0.1 - 40.0).abs() < 1.0, "{}", total(&segs) * 0.1);
        assert!(segs.iter().all(|(a, b)| [a, b].iter().all(|p| p.x > -1e-6 && p.x < 10.0 + 1e-6 && p.y > -1e-6 && p.y < 4.0 + 1e-6)));
    }

    #[test]
    fn caps_and_empty_input() {
        assert!(scan(&[], 1.0, 0.0, true).unwrap().is_empty());
        let open = [Shape { polys: vec![vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)]], rule: FillRule::NonZero }];
        assert!(scan(&open, 1.0, 0.0, true).unwrap().is_empty());
        let huge = [Shape { polys: vec![rect(0.0, 0.0, 10.0, 1.0e6, true)], rule: FillRule::NonZero }];
        assert!(scan(&huge, 0.01, 0.0, true).is_err());
        assert!(scan(&huge, f64::NAN, 0.0, true).is_err());
    }
}
