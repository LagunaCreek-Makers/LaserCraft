//! Cut order within a layer: a contour is cut only once every contour inside it is cut (so parts
//! don't drop out of the sheet before their holes are cut), and among those ready the nearest one
//! comes next (greedy nearest neighbour), to shorten travel. Closed contours start at their vertex
//! nearest the head; open ones may run backwards.

use vectorcraft_geom::{Point, Rect};

use crate::walk::Contour;

fn bounds(points: &[Point]) -> Rect {
    let mut r = Rect::from_points(points.first().copied().unwrap_or(Point::ZERO), points.first().copied().unwrap_or(Point::ZERO));
    for p in points {
        r = r.union_pt(*p);
    }
    r
}

/// Is `p` inside the polygon `poly` (even-odd)?
fn inside(poly: &[Point], p: Point) -> bool {
    let mut inside = false;
    let Some(mut prev) = poly.last().copied() else { return false };
    for &q in poly {
        if (q.y > p.y) != (prev.y > p.y) && p.x < (prev.x - q.x) * (p.y - q.y) / (prev.y - q.y) + q.x {
            inside = !inside;
        }
        prev = q;
    }
    inside
}

/// Does closed contour `outer` (with bounds `ob`) hold `inner` (with bounds `ib`)? Inner's bounds
/// lie within outer's and its points (most of them, for curves that touch) are inside it.
fn holds(outer: &Contour, ob: Rect, inner: &Contour, ib: Rect) -> bool {
    if !outer.closed || ib.x0 < ob.x0 || ib.y0 < ob.y0 || ib.x1 > ob.x1 || ib.y1 > ob.y1 || ib.area() >= ob.area() {
        return false;
    }
    let n = inner.points.len();
    let step = (n / 16).max(1);
    let (mut yes, mut all) = (0usize, 0usize);
    for p in inner.points.iter().step_by(step) {
        all += 1;
        if inside(&outer.points, *p) {
            yes += 1;
        }
    }
    yes * 2 > all
}

/// `contours` in cutting order, starting with the head at `start`; returns where the head ends.
pub(crate) fn order(contours: Vec<Contour>, start: Point) -> (Vec<Contour>, Point) {
    let n = contours.len();
    let boxes: Vec<Rect> = contours.iter().map(|c| bounds(&c.points)).collect();
    // How many uncut contours each one holds.
    let mut waiting = vec![0usize; n];
    // The contours holding each one.
    let mut held_by: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, (outer, ob)) in contours.iter().zip(&boxes).enumerate() {
        if !outer.closed {
            continue;
        }
        for (j, (inner, ib)) in contours.iter().zip(&boxes).enumerate() {
            if i != j && holds(outer, *ob, inner, *ib) {
                if let Some(w) = waiting.get_mut(i) {
                    *w += 1;
                }
                if let Some(h) = held_by.get_mut(j) {
                    h.push(i);
                }
            }
        }
    }
    let mut slots: Vec<Option<Contour>> = contours.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(n);
    let mut head = start;
    while out.len() < n {
        // The nearest ready contour: closed ones measured to their bounds (cheap, and close to
        // the nearest vertex), open ones to either end.
        let mut best: Option<(f64, usize, bool)> = None;
        for (i, c) in slots.iter().enumerate() {
            let Some(c) = c else { continue };
            if waiting.get(i).copied().unwrap_or(0) > 0 {
                continue;
            }
            let ends = if c.closed {
                let b = boxes.get(i).copied().unwrap_or(Rect::ZERO);
                let nearest = Point::new(head.x.clamp(b.x0, b.x1), head.y.clamp(b.y0, b.y1));
                [(nearest.distance_squared(head), false), (f64::INFINITY, false)]
            } else {
                let d = |p: Option<&Point>| p.map_or(f64::INFINITY, |p| p.distance_squared(head));
                [(d(c.points.first()), false), (d(c.points.last()), true)]
            };
            for (d, rev) in ends {
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, i, rev));
                }
            }
        }
        // Nothing ready would mean contours holding each other: cut what is left as it comes.
        let (i, rev) = match best {
            Some((_, i, rev)) => (i, rev),
            None => match slots.iter().position(Option::is_some) {
                Some(i) => (i, false),
                None => break,
            },
        };
        let Some(mut c) = slots.get_mut(i).and_then(Option::take) else { break };
        if c.closed {
            // Enter at the vertex nearest the head.
            let k =
                c.points.iter().enumerate().min_by(|a, b| a.1.distance_squared(head).total_cmp(&b.1.distance_squared(head))).map_or(0, |(k, _)| k);
            c.points.rotate_left(k);
        } else if rev {
            c.points.reverse();
        }
        head = if c.closed { c.points.first() } else { c.points.last() }.copied().unwrap_or(head);
        for &h in held_by.get(i).into_iter().flatten() {
            if let Some(w) = waiting.get_mut(h) {
                *w = w.saturating_sub(1);
            }
        }
        out.push(c);
    }
    (out, head)
}
