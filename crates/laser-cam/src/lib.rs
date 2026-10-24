//! LaserCraft CAM: a VectorCraft [`Document`] as a controller-independent [`lasercraft_core::Job`].
//!
//! Live effects are baked first ([`vectorcraft_effects::bake_document`]); then each visible,
//! printable layer's paths are flattened to polylines within a tolerance, converted from points to
//! millimetres and placed on the bed from the machine's origin corner ([`Origin`]: each axis runs
//! from that corner of the artboard into it). Each layer is cut with its [`LayerSettings`], layers in
//! paint order (bottom first), inner contours before the outlines around them and otherwise
//! nearest first. Nothing here talks to a machine.

mod fill;
pub mod hooks;
pub mod machine;
mod order;
pub mod raster;
pub mod settings;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use lasercraft_core::{Job, Operation, Point as MmPoint};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::{Affine, Point, Rect};

pub use machine::{MachineProfile, Origin, default_profiles_path, find_machine, machines};
pub use settings::{Dither, LAYER_SETTINGS_KEY, LaserMode, LayerSettings, layer_settings, set_layer_settings};

/// Millimetres per point.
pub const MM_PER_PT: f64 = 25.4 / 72.0;
/// The finest and coarsest flattening tolerance (mm).
pub const TOLERANCE_RANGE: std::ops::RangeInclusive<f64> = 0.001..=5.0;

/// How a document becomes a job.
#[derive(Clone, Debug)]
pub struct JobOptions {
    /// Curves are flattened to within this distance (mm).
    pub tolerance_mm: f64,
    pub origin: Origin,
    /// The artboard whose corner is the bed origin (the art's bounds when the document has none).
    pub artboard: usize,
    /// Only these objects (and what they hold); `None`: everything.
    pub selection: Option<Vec<NodeId>>,
    /// Settings by layer id; `None`: the settings the document holds ([`layer_settings`]).
    pub layers: Option<BTreeMap<NodeId, LayerSettings>>,
    /// The settings of layers that have none.
    pub default_layer: LayerSettings,
    /// The bed (width, height mm): Fill overscan stops at its edges. `None`: only at 0.
    pub bed: Option<(f64, f64)>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            tolerance_mm: 0.05,
            origin: Origin::BottomLeft,
            artboard: 0,
            selection: None,
            layers: None,
            default_layer: LayerSettings::default(),
            bed: None,
        }
    }
}

/// A job and what was left out of it.
#[derive(Clone, Debug)]
pub struct JobOutput {
    pub job: Job,
    /// One line for each kind of art left out or changed (each said once).
    pub warnings: Vec<String>,
    /// How many polylines are cut (each pass counted).
    pub contours: usize,
}

/// The transform from document space (points, y down) to bed millimetres for `origin` on `frame`.
pub fn bed_transform(frame: Rect, origin: Origin) -> Affine {
    let k = MM_PER_PT;
    // x from the left edge rightwards or the right edge leftwards; y from the bottom edge up or
    // the top edge down.
    let (sx, tx) = if origin.right() { (-k, frame.x1 * k) } else { (k, -frame.x0 * k) };
    let (sy, ty) = if origin.top() { (k, -frame.y0 * k) } else { (-k, frame.y1 * k) };
    Affine::new([sx, 0.0, 0.0, sy, tx, ty])
}

/// `doc` as a laser job with `o`.
pub fn document_to_job(doc: &Document, o: &JobOptions) -> Result<JobOutput, String> {
    if !(o.tolerance_mm.is_finite() && TOLERANCE_RANGE.contains(&o.tolerance_mm)) {
        return Err(format!("tolerance must be from {} to {} mm, not {}", TOLERANCE_RANGE.start(), TOLERANCE_RANGE.end(), o.tolerance_mm));
    }
    o.default_layer.validate()?;
    let frame = match doc.artboards.get(o.artboard) {
        Some(a) => a.rect,
        None if doc.artboards.is_empty() => doc.art_bounds().unwrap_or(Rect::ZERO),
        None => return Err(format!("no artboard {} (the document has {})", o.artboard.saturating_add(1), doc.artboards.len())),
    };
    let baked = vectorcraft_effects::bake_document(doc);
    let doc = baked.as_ref().unwrap_or(doc);
    let stored;
    let settings = match &o.layers {
        Some(l) => l,
        None => {
            stored = layer_settings(doc);
            &stored
        }
    };
    for (id, s) in settings {
        s.validate().map_err(|e| format!("layer {id}: {e}"))?;
    }
    let walked = walk::Walker::new(doc, bed_transform(frame, o.origin), o.tolerance_mm, settings, o.default_layer, o.selection.as_deref()).run();
    let mut warnings = walked.warnings;
    let mut ops = vec![];
    let mut head = Point::ZERO;
    let mut contours = 0;
    let mut shortened = false;
    for layer in walked.layers {
        let s = layer.settings;
        if !layer.contours.is_empty() {
            let (list, end) = order::order(layer.contours, head);
            head = end;
            for _ in 0..s.passes {
                for c in &list {
                    emit(c, &s, &mut ops);
                    contours += 1;
                }
            }
        }
        if !layer.shapes.is_empty() {
            let mut segs = fill::scan(&layer.shapes, s.interval_mm, s.angle_deg, s.bidirectional)?;
            if s.crosshatch {
                segs.extend(fill::scan(&layer.shapes, s.interval_mm, s.angle_deg + 90.0, s.bidirectional)?);
            }
            for _ in 0..s.passes {
                for seg in &segs {
                    shortened |= emit_scan(*seg, &s, o.bed, &mut ops);
                    contours += 1;
                }
            }
            if let Some(last) = segs.last() {
                head = last.1;
            }
        }
        for img in &layer.images {
            match engrave_image(img, &s, o.bed, &mut ops)? {
                Some((rows, end, short)) => {
                    contours += rows;
                    head = end;
                    shortened |= short;
                }
                None => {
                    if !warnings.iter().any(|w| w == walk::UNREADABLE) {
                        warnings.push(walk::UNREADABLE.into());
                    }
                }
            }
            if ops.len() > walk::MAX_POINTS {
                return Err("the job is too large: use a larger image interval".into());
            }
        }
        if ops.len() > walk::MAX_POINTS {
            return Err("the job is too large: use a larger fill interval or fewer passes".into());
        }
    }
    if shortened {
        warnings.push(OVERSCAN_SHORTENED.into());
    }
    Ok(JobOutput { job: Job { operations: ops }, warnings, contours })
}

const OVERSCAN_SHORTENED: &str = "fill overscan was shortened where it would run off the bed";

/// How far from `p` the head may run along `d` (a unit vector) before leaving the bed (0..w,
/// 0..h; `None`: only 0 is an edge).
fn room(p: Point, d: Point, bed: Option<(f64, f64)>) -> f64 {
    let (w, h) = bed.unwrap_or((f64::INFINITY, f64::INFINITY));
    let along = |pos: f64, dir: f64, max: f64| {
        if dir < -1e-12 {
            pos / -dir
        } else if dir > 1e-12 {
            (max - pos) / dir
        } else {
            f64::INFINITY
        }
    };
    along(p.x, d.x, w).min(along(p.y, d.y, h)).max(0.0)
}

/// One scan line: travel to its overscan start, run up to speed with the laser at 0, burn the
/// stretch, run out at 0. Returns whether the overscan was shortened at the bed's edge.
fn emit_scan((a, b): (Point, Point), s: &LayerSettings, bed: Option<(f64, f64)>, ops: &mut Vec<Operation>) -> bool {
    let mm = |p: Point| MmPoint { x_mm: p.x, y_mm: p.y };
    let at = |p: Point, power: f64| Operation::Cut { to: mm(p), speed_mm_min: s.speed_mm_min, power_percent: power };
    let len = a.distance(b);
    let os = s.overscan_mm;
    if os <= 0.0 || len <= 1e-12 {
        ops.extend([Operation::Travel(mm(a)), at(b, s.power_percent), Operation::LaserOff]);
        return false;
    }
    let d = Point::new((b.x - a.x) / len, (b.y - a.y) / len);
    let before = os.min(room(a, Point::new(-d.x, -d.y), bed));
    let after = os.min(room(b, d, bed));
    let start = Point::new(a.x - d.x * before, a.y - d.y * before);
    let end = Point::new(b.x + d.x * after, b.y + d.y * after);
    ops.push(Operation::Travel(mm(start)));
    if before > 1e-9 {
        ops.push(at(a, 0.0));
    }
    ops.push(at(b, s.power_percent));
    if after > 1e-9 {
        ops.push(at(end, 0.0));
    }
    ops.push(Operation::LaserOff);
    before < os - 1e-9 || after < os - 1e-9
}

/// One placed image engraved row by row (`passes` times): `None` when it can't be decoded, else
/// the rows burnt, where the head ends and whether an overscan was shortened.
fn engrave_image(
    img: &walk::RasterImage,
    s: &LayerSettings,
    bed: Option<(f64, f64)>,
    ops: &mut Vec<Operation>,
) -> Result<Option<(usize, Point, bool)>, String> {
    let o = img.to_mm * Point::ZERO;
    let u = img.to_mm * Point::new(f64::from(img.width), 0.0) - o;
    let v = img.to_mm * Point::new(0.0, f64::from(img.height)) - o;
    let (cols, rows) = raster::grid((u.hypot(), v.hypot()), s.interval_mm)?;
    // Samples at the crossings of a (rows - 1) × (cols - 1) grid laid over the image.
    let (gr, gc) = (rows.saturating_sub(1).max(1), cols.saturating_sub(1).max(1));
    let Some(samples) = img.blob.grid_samples(gr, gc) else { return Ok(None) };
    let (w, h) = (gc as usize + 1, gr as usize + 1);
    let lv = raster::levels(&raster::darkness(&samples, s.negative), w, h, s.dither);
    let mm = |p: Point| MmPoint { x_mm: p.x, y_mm: p.y };
    let at = |p: Point, power: f64| Operation::Cut { to: mm(p), speed_mm_min: s.speed_mm_min, power_percent: power };
    let len = u.hypot();
    let dir = if len > 1e-12 { Point::new(u.x / len, u.y / len) } else { Point::new(1.0, 0.0) };
    let mut burnt = 0;
    let mut head = Point::ZERO;
    let mut shortened = false;
    for _ in 0..s.passes {
        let mut row_n = 0usize;
        for y in 0..h {
            let row: Vec<Option<f64>> =
                (0..w).map(|x| raster::power(lv.get(y * w + x).copied().unwrap_or(0.0), s.min_power_percent, s.power_percent)).collect();
            let (Some(first), Some(last)) = (row.iter().position(Option::is_some), row.iter().rposition(Option::is_some)) else { continue };
            // Sample x covers [x, x + 1) / w of the row; the row lies at (y + 0.5) / h down the image.
            let p = |t: f64| Point::new(o.x + u.x * t + v.x * (y as f64 + 0.5) / h as f64, o.y + u.y * t + v.y * (y as f64 + 0.5) / h as f64);
            let t = |k: usize| k as f64 / w as f64;
            // Runs of equal power from the first burning sample to the last.
            let mut runs: Vec<(f64, f64, f64)> = vec![];
            for (k, pw) in row.iter().enumerate().take(last + 1).skip(first) {
                let pw = pw.unwrap_or(0.0);
                match runs.last_mut() {
                    Some(r) if (r.2 - pw).abs() < 1e-9 => r.1 = t(k + 1),
                    _ => runs.push((t(k), t(k + 1), pw)),
                }
            }
            let back = s.bidirectional && row_n % 2 == 1;
            row_n += 1;
            if back {
                runs.reverse();
                for r in &mut runs {
                    *r = (r.1, r.0, r.2);
                }
            }
            let (Some(start), Some(end)) = (runs.first().map(|r| p(r.0)), runs.last().map(|r| p(r.1))) else { continue };
            let d = if back { Point::new(-dir.x, -dir.y) } else { dir };
            let before = s.overscan_mm.min(room(start, Point::new(-d.x, -d.y), bed));
            let after = s.overscan_mm.min(room(end, d, bed));
            shortened |= before < s.overscan_mm - 1e-9 || after < s.overscan_mm - 1e-9;
            ops.push(Operation::Travel(mm(Point::new(start.x - d.x * before, start.y - d.y * before))));
            if before > 1e-9 {
                ops.push(at(start, 0.0));
            }
            for r in &runs {
                ops.push(at(p(r.1), r.2));
            }
            if after > 1e-9 {
                ops.push(at(Point::new(end.x + d.x * after, end.y + d.y * after), 0.0));
            }
            ops.push(Operation::LaserOff);
            burnt += 1;
            head = end;
            if ops.len() > walk::MAX_POINTS {
                return Err("the job is too large: use a larger image interval".into());
            }
        }
    }
    Ok(Some((burnt, head, shortened)))
}

fn emit(c: &walk::Contour, s: &LayerSettings, ops: &mut Vec<Operation>) {
    let mm = |p: &Point| MmPoint { x_mm: p.x, y_mm: p.y };
    let Some((first, rest)) = c.points.split_first() else { return };
    let cut = |p: &Point| Operation::Cut { to: mm(p), speed_mm_min: s.speed_mm_min, power_percent: s.power_percent };
    ops.push(Operation::Travel(mm(first)));
    ops.extend(rest.iter().map(cut));
    if c.closed {
        ops.push(cut(first));
    }
    ops.push(Operation::LaserOff);
}
