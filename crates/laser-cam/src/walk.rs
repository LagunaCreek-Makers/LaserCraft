//! The document walk: every visible, printable path becomes polylines in bed millimetres, tagged
//! with the layer whose settings cut it. Modelled on the DXF writer's walk (`vectorcraft-cad`):
//! layers in paint order, template layers, guides and hidden art left out, live art expanded.

use std::collections::BTreeMap;
use std::sync::Arc;

use kurbo::PathEl;
use vectorcraft_color::Paint;
use vectorcraft_doc::{Document, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, BezPath, Point};

use crate::settings::LayerSettings;

/// Symbols inside symbols are followed this deep.
const MAX_NEST: u32 = 16;
/// Most points one job may hold (a cap on what a damaged or huge file makes us allocate).
pub(crate) const MAX_POINTS: usize = 20_000_000;

pub(crate) const IMAGES: &str = "placed images are left out: Line mode cuts paths only";
pub(crate) const TEXT: &str = "type is left out: outline it first (Type → Create Outlines)";
pub(crate) const GRADIENTS: &str = "gradient paint is ignored: Line mode cuts the path's outline";
pub(crate) const CLIPPING: &str = "clipping masks are not applied: the art they clip is cut whole, the clipping path is left out";
pub(crate) const TOO_MANY: &str = "the job has too many points: the rest of the art is left out";

/// One polyline to cut, in bed millimetres.
#[derive(Clone, Debug)]
pub(crate) struct Contour {
    pub points: Vec<Point>,
    /// The cut returns to the first point.
    pub closed: bool,
}

/// What the walk found: the contours of each settings layer, in paint order.
pub(crate) struct Walked {
    /// (layer id, its settings, its contours).
    pub layers: Vec<(NodeId, LayerSettings, Vec<Contour>)>,
    pub warnings: Vec<String>,
}

pub(crate) struct Walker<'a> {
    doc: &'a Document,
    /// Document space → bed millimetres.
    to_mm: Affine,
    tolerance_mm: f64,
    settings: &'a BTreeMap<NodeId, LayerSettings>,
    default: LayerSettings,
    /// Only these objects (and what they hold), when given.
    only: Option<&'a [NodeId]>,
    out: Walked,
    /// Index into `out.layers` the art goes to.
    current: Option<usize>,
    points: usize,
    nest: u32,
}

impl<'a> Walker<'a> {
    pub fn new(
        doc: &'a Document,
        to_mm: Affine,
        tolerance_mm: f64,
        settings: &'a BTreeMap<NodeId, LayerSettings>,
        default: LayerSettings,
        only: Option<&'a [NodeId]>,
    ) -> Self {
        Self {
            doc,
            to_mm,
            tolerance_mm,
            settings,
            default,
            only,
            out: Walked { layers: vec![], warnings: vec![] },
            current: None,
            points: 0,
            nest: 0,
        }
    }

    pub fn run(mut self) -> Walked {
        let doc = self.doc;
        for l in &doc.layers {
            self.node(l, self.only.is_none());
        }
        self.out.layers.retain(|(_, s, c)| s.enabled && !c.is_empty());
        self.out
    }

    fn warn(&mut self, w: &str) {
        if !self.out.warnings.iter().any(|x| x == w) {
            self.out.warnings.push(w.to_string());
        }
    }

    /// `n` and what it holds; `chosen`: inside an object the selection names (or no selection).
    fn node(&mut self, n: &Node, chosen: bool) {
        if !n.visible {
            return;
        }
        let chosen = chosen || self.only.is_some_and(|ids| ids.contains(&n.id));
        match &n.kind {
            NodeKind::Layer { template: true, .. } | NodeKind::Layer { printable: false, .. } => {}
            NodeKind::Layer { children, clip, .. } => {
                let outer = self.current;
                // A sublayer with settings of its own cuts its art with them; one without cuts
                // with its parent layer's (the defaults at the top).
                if self.current.is_none() || self.settings.contains_key(&n.id) {
                    let s = self.settings.get(&n.id).copied().unwrap_or(self.default);
                    self.out.layers.push((n.id, s, vec![]));
                    self.current = Some(self.out.layers.len() - 1);
                }
                self.children(children, *clip, chosen);
                self.current = outer;
            }
            NodeKind::Group { children, clip } => self.children(children, *clip, chosen),
            NodeKind::Path { path, guide, clipping, .. } => {
                if chosen && !*guide && !*clipping {
                    self.note_paint(n);
                    self.cut(&path.to_bezpath());
                }
            }
            NodeKind::Compound { children, .. } => {
                if chosen {
                    self.note_paint(n);
                    let mut bp = BezPath::new();
                    for p in children.iter().filter_map(|c| c.path_data()) {
                        bp.extend(p.to_bezpath());
                    }
                    self.cut(&bp);
                }
            }
            NodeKind::Text(_) => {
                if chosen {
                    self.warn(TEXT);
                }
            }
            NodeKind::Image(_) => {
                if chosen {
                    self.warn(IMAGES);
                }
            }
            NodeKind::SymbolInstance { symbol, xf } => {
                if let Some(sym) = self.doc.symbols.iter().find(|s| &s.name == symbol)
                    && self.nest < MAX_NEST
                {
                    let mut art = (*sym.art).clone();
                    art.transform(*xf, false);
                    // The symbol's art is visible wherever the instance is.
                    art.visible = true;
                    self.nest += 1;
                    self.node(&art, chosen);
                    self.nest -= 1;
                }
            }
            // Live blends, envelopes, meshes and repeats are cut as their evaluated art.
            NodeKind::Blend { .. }
            | NodeKind::Envelope { .. }
            | NodeKind::Mesh(_)
            | NodeKind::Repeat(_)
            | NodeKind::PlacedDocument(_)
            | NodeKind::Intertwine { .. } => {
                if self.nest < MAX_NEST {
                    let g = vectorcraft_effects::expand_live_deep(Some(self.doc), n);
                    self.nest += 1;
                    for c in g.children().into_iter().flatten() {
                        self.node(c, chosen);
                    }
                    self.nest -= 1;
                }
            }
        }
    }

    /// A container's children; a clipping container's first child is its clipping path, which
    /// isn't cut.
    fn children(&mut self, children: &[Arc<Node>], clip: bool, chosen: bool) {
        let rest = match children.split_first() {
            Some((_, rest)) if clip => {
                self.warn(CLIPPING);
                rest
            }
            _ => children,
        };
        for c in rest {
            self.node(c, chosen);
        }
    }

    fn note_paint(&mut self, n: &Node) {
        if n.appearance.items.iter().any(|i| matches!(i.paint(), Paint::Gradient(_))) {
            self.warn(GRADIENTS);
        }
    }

    /// `bp` (document space) flattened into polylines in millimetres, added to the current layer.
    fn cut(&mut self, bp: &BezPath) {
        let Some(layer) = self.current else { return };
        let mm = self.to_mm * bp.clone();
        let mut contours: Vec<Contour> = vec![];
        let mut cur: Vec<Point> = vec![];
        let finish = |cur: &mut Vec<Point>, closed: bool, out: &mut Vec<Contour>| {
            let mut pts = std::mem::take(cur);
            if closed && pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.distance(*b) <= 1e-9) {
                pts.pop();
            }
            if pts.len() >= 2 {
                out.push(Contour { points: pts, closed });
            }
        };
        let push = |p: Point, cur: &mut Vec<Point>| {
            if p.is_finite() && cur.last().is_none_or(|q| q.distance(p) > 1e-9) {
                cur.push(p);
            }
        };
        kurbo::flatten(mm.iter(), self.tolerance_mm, |el| match el {
            PathEl::MoveTo(p) => {
                finish(&mut cur, false, &mut contours);
                push(p, &mut cur);
            }
            PathEl::LineTo(p) => push(p, &mut cur),
            PathEl::ClosePath => finish(&mut cur, true, &mut contours),
            _ => {}
        });
        finish(&mut cur, false, &mut contours);
        let n: usize = contours.iter().map(|c| c.points.len()).sum();
        if self.points.saturating_add(n) > MAX_POINTS {
            self.warn(TOO_MANY);
            return;
        }
        self.points += n;
        if let Some((_, _, list)) = self.out.layers.get_mut(layer) {
            list.extend(contours);
        }
    }
}
