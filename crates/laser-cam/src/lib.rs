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
pub mod settings;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use lasercraft_core::{Job, Operation, Point as MmPoint};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::{Affine, Point, Rect};

pub use machine::{MachineProfile, Origin, default_profiles_path, find_machine, machines};
pub use settings::{LAYER_SETTINGS_KEY, LaserMode, LayerSettings, layer_settings, set_layer_settings};

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
