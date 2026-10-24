//! The laser panel's document section: the job made from the open document by the engine's
//! `laser.exportGcode`, on the bed of the loaded machine profile, and the per-layer cut settings
//! (`laser.layers`, `laser.setLayer`). Everything goes through commands (the app's `run`); this
//! module never touches the document or a machine itself.

use std::collections::BTreeMap;

use egui::{Pos2, Rect};
use lasercraft_cam::{Dither, LaserMode, Origin};
use serde_json::{Value, json};
use vectorcraft_ui_egui::theme::Tokens;

/// Runs a command by id with its params (the app's `VectorcraftApp::run`).
pub type Run<'a> = dyn FnMut(&str, Value) -> Result<Value, String> + 'a;

/// The loaded machine profile: the bed a job must fit, the S value of full power, the corner
/// the machine counts from, and the air-assist and accessory G-code around each job
/// (`lasercraft_cam::hooks::job_hooks`).
#[derive(Clone, Debug, PartialEq)]
pub struct Bed {
    pub width_mm: f64,
    pub height_mm: f64,
    pub max_s: u32,
    pub origin: Origin,
    pub prologue: Vec<String>,
    pub epilogue: Vec<String>,
}

/// Where machine point (`x`, `y`) mm lies on screen when the bed (`w` × `h` mm, centred on
/// `center` at `sx`, `sy` pixels per mm) is drawn as seen from above, front edge at the bottom:
/// x runs right from a left origin and left from a right one, y up from a bottom origin and down
/// from a top one.
pub fn bed_to_screen(center: Pos2, (sx, sy): (f32, f32), (w, h): (f64, f64), origin: Origin, x: f64, y: f64) -> Pos2 {
    let right = matches!(origin, Origin::TopRight | Origin::BottomRight);
    let top = matches!(origin, Origin::TopLeft | Origin::TopRight);
    let dx = (x - w / 2.0) as f32 * sx;
    let dy = (y - h / 2.0) as f32 * sy;
    egui::pos2(if right { center.x - dx } else { center.x + dx }, if top { center.y + dy } else { center.y - dy })
}

/// The params of `laser.exportGcode` for `bed` (none: the command asks for a machine).
pub fn export_params(bed: Option<&Bed>, selection_only: bool, tolerance: f64, path: Option<&str>) -> Value {
    let mut p = json!({ "selectionOnly": selection_only, "tolerance": tolerance });
    if let Some(b) = bed {
        p["bedWidth"] = json!(b.width_mm);
        p["bedHeight"] = json!(b.height_mm);
        p["maxS"] = json!(b.max_s);
        p["origin"] = json!(b.origin.id());
        p["prologue"] = json!(b.prologue);
        p["epilogue"] = json!(b.epilogue);
    }
    if let Some(path) = path {
        p["path"] = json!(path);
    }
    p
}

/// One document layer as `laser.layers` gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    pub id: u64,
    pub name: String,
    pub speed: f64,
    pub power: f64,
    pub passes: u32,
    pub enabled: bool,
    pub mode: LaserMode,
    /// Fill: line spacing (mm), angle (degrees), back-and-forth lines, overscan (mm), crosshatch.
    pub interval: f64,
    pub angle: f64,
    pub bidirectional: bool,
    pub overscan: f64,
    pub crosshatch: bool,
    /// Image: dithering, the lightest grey's power (%), burn the light parts.
    pub dither: Dither,
    pub min_power: f64,
    pub negative: bool,
    /// The layer has settings of its own (else it cuts with its parent's or the defaults).
    pub custom: bool,
}

impl LayerRow {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            id: v["layer"].as_u64()?,
            name: v["name"].as_str().unwrap_or("").to_string(),
            speed: v["speed"].as_f64()?,
            power: v["power"].as_f64()?,
            passes: u32::try_from(v["passes"].as_u64()?).ok()?,
            enabled: v["enabled"].as_bool()?,
            mode: v["mode"].as_str().and_then(LaserMode::parse).unwrap_or_default(),
            interval: v["interval"].as_f64().unwrap_or(0.1),
            angle: v["angle"].as_f64().unwrap_or(0.0),
            bidirectional: v["bidirectional"].as_bool().unwrap_or(true),
            overscan: v["overscan"].as_f64().unwrap_or(0.0),
            crosshatch: v["crosshatch"].as_bool().unwrap_or(false),
            dither: v["dither"].as_str().and_then(Dither::parse).unwrap_or_default(),
            min_power: v["minPower"].as_f64().unwrap_or(0.0),
            negative: v["negative"].as_bool().unwrap_or(false),
            custom: v["custom"].as_bool().unwrap_or(false),
        })
    }
    fn params(&self) -> Value {
        json!({
            "layer": self.id,
            "speed": self.speed,
            "power": self.power,
            "passes": self.passes,
            "enabled": self.enabled,
            "mode": self.mode.id(),
            "interval": self.interval,
            "angle": self.angle,
            "bidirectional": self.bidirectional,
            "overscan": self.overscan,
            "crosshatch": self.crosshatch,
            "dither": self.dither.id(),
            "minPower": self.min_power,
            "negative": self.negative,
        })
    }
}

/// A job made from the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub gcode: String,
    pub contours: u64,
    /// The file it was written to.
    pub path: Option<String>,
}

/// What the section asks the panel to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A job was made: show it in the preview and dry run.
    Generated(Generated),
    /// Ask where to save the G-code, then call [`DocExport::generate`] with the path.
    ChooseExportPath,
}

/// The document section's state.
pub struct DocExport {
    pub selection_only: bool,
    /// Curves are flattened to within this (mm).
    pub tolerance: f64,
    /// What the last job left out or changed.
    pub warnings: Vec<String>,
    /// The last command's error.
    pub error: Option<String>,
    /// What the last command did.
    pub status: String,
    pub layers: Vec<LayerRow>,
    /// Layer settings typed but not applied yet, by layer id.
    edits: BTreeMap<u64, LayerRow>,
}

impl Default for DocExport {
    fn default() -> Self {
        Self { selection_only: false, tolerance: 0.05, warnings: vec![], error: None, status: String::new(), layers: vec![], edits: BTreeMap::new() }
    }
}

impl DocExport {
    /// Make the job from the document on `bed` (written to `path` when given). The command's
    /// error (no machine, art off the bed, nothing to cut…) is kept to show.
    pub fn generate(&mut self, run: &mut Run, bed: Option<&Bed>, path: Option<&str>) -> Option<Generated> {
        match run("laser.exportGcode", export_params(bed, self.selection_only, self.tolerance, path)) {
            Ok(r) => {
                self.error = None;
                self.warnings = r["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                let g = Generated {
                    gcode: r["gcode"].as_str().unwrap_or("").to_string(),
                    contours: r["contours"].as_u64().unwrap_or(0),
                    path: r["path"].as_str().map(str::to_string),
                };
                self.status = match &g.path {
                    Some(p) => format!("Wrote {} ({} cuts). Review it before running it.", p, g.contours),
                    None => format!("{} cuts from the document. Review the dry run before running it.", g.contours),
                };
                Some(g)
            }
            Err(e) => {
                self.warnings.clear();
                self.status.clear();
                self.error = Some(e);
                None
            }
        }
    }

    /// Read the document's layers and their settings again.
    pub fn refresh_layers(&mut self, run: &mut Run) {
        match run("laser.layers", json!({})) {
            Ok(r) => {
                self.layers = r["layers"].as_array().into_iter().flatten().filter_map(LayerRow::from_json).collect();
                let ids: Vec<u64> = self.layers.iter().map(|l| l.id).collect();
                self.edits.retain(|id, _| ids.contains(id));
            }
            Err(_) => self.layers.clear(),
        }
    }

    /// Set layer `row`'s settings (one undoable step).
    pub fn apply_layer(&mut self, run: &mut Run, row: &LayerRow) {
        match run("laser.setLayer", row.params()) {
            Ok(_) => {
                self.edits.remove(&row.id);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.refresh_layers(run);
    }

    /// The section: the machine the job is made for, Generate and Export, what was left out, and
    /// the layers' settings. `machine` names the loaded profile.
    pub fn ui(&mut self, ui: &mut egui::Ui, run: &mut Run, bed: Option<&Bed>, machine: &str) -> Option<Action> {
        let t = Tokens::get(ui.ctx());
        let mut action = None;
        match bed {
            Some(b) => {
                ui.label(format!("Machine: {machine} — {:.0} × {:.0} mm bed, origin {}", b.width_mm, b.height_mm, b.origin.label().to_lowercase()));
            }
            None => {
                ui.colored_label(t.warning, "No machine profile loaded: choose one under Saved machines and click Load machine.");
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.selection_only, "Selected art only");
            ui.label("Curve tolerance");
            ui.add(egui::DragValue::new(&mut self.tolerance).range(0.001..=5.0).speed(0.005).suffix(" mm"));
        });
        ui.horizontal(|ui| {
            if ui.button("Generate from document").clicked()
                && let Some(g) = self.generate(run, bed, None)
            {
                action = Some(Action::Generated(g));
            }
            if ui.button("Export G-code…").clicked() {
                action = Some(Action::ChooseExportPath);
            }
        });
        if let Some(e) = &self.error {
            ui.colored_label(t.error, e);
        }
        for w in &self.warnings {
            ui.colored_label(t.warning, format!("⚠ {w}"));
        }
        if !self.status.is_empty() {
            ui.small(&self.status);
        }
        ui.small("The machine's air-assist and accessory G-code wraps the job, in the dry run and in the exported file alike.");
        egui::CollapsingHeader::new("Document layers").default_open(true).show(ui, |ui| {
            self.refresh_layers(run);
            if self.layers.is_empty() {
                ui.small("No document is open.");
                return;
            }
            let mut apply: Option<LayerRow> = None;
            egui::Grid::new("lasercraft_doc_layers").striped(true).show(ui, |ui| {
                for h in ["Layer", "On", "Mode", "Speed (mm/min)", "Power (%)", "Passes", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for row in &self.layers {
                    let mut e = self.edits.get(&row.id).cloned().unwrap_or_else(|| row.clone());
                    let label = if row.custom { row.name.clone() } else { format!("{} (default)", row.name) };
                    ui.label(if label.is_empty() { format!("Layer {}", row.id) } else { label });
                    // On/off applies at once; the numbers when Apply is clicked (one undo step).
                    if ui.checkbox(&mut e.enabled, "").changed() {
                        apply = Some(LayerRow { enabled: e.enabled, ..row.clone() });
                    }
                    let mut changed = false;
                    egui::ComboBox::from_id_salt(("lasercraft_layer_mode", row.id)).selected_text(e.mode.label()).show_ui(ui, |ui| {
                        for m in LaserMode::ALL {
                            changed |= ui.selectable_value(&mut e.mode, m, m.label()).changed();
                        }
                    });
                    changed |= ui.add(egui::DragValue::new(&mut e.speed).range(1.0..=100_000.0).speed(10.0)).changed();
                    changed |= ui.add(egui::DragValue::new(&mut e.power).range(0.0..=100.0).speed(0.5)).changed();
                    changed |= ui.add(egui::DragValue::new(&mut e.passes).range(1..=100)).changed();
                    let dirty = e != *row;
                    if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
                        apply = Some(e.clone());
                    }
                    ui.end_row();
                    if e.mode == LaserMode::Fill {
                        // The fill's own settings, on a row under the layer's.
                        ui.label("");
                        ui.label("");
                        ui.label("Fill");
                        changed |= ui.add(egui::DragValue::new(&mut e.interval).range(0.01..=10.0).speed(0.005).prefix("every ").suffix(" mm")).changed();
                        changed |= ui.add(egui::DragValue::new(&mut e.angle).range(-360.0..=360.0).speed(1.0).suffix("°")).changed();
                        changed |= ui.add(egui::DragValue::new(&mut e.overscan).range(0.0..=50.0).speed(0.1).prefix("overscan ").suffix(" mm")).changed();
                        ui.horizontal(|ui| {
                            changed |= ui.checkbox(&mut e.bidirectional, "Both ways").changed();
                            changed |= ui.checkbox(&mut e.crosshatch, "Crosshatch").changed();
                        });
                        ui.end_row();
                    }
                    if e.mode == LaserMode::Image {
                        // The image's own settings, on a row under the layer's.
                        ui.label("");
                        ui.label("");
                        ui.label("Image");
                        egui::ComboBox::from_id_salt(("lasercraft_layer_dither", row.id)).selected_text(e.dither.label()).show_ui(ui, |ui| {
                            for d in Dither::ALL {
                                changed |= ui.selectable_value(&mut e.dither, d, d.label()).changed();
                            }
                        });
                        changed |= ui.add(egui::DragValue::new(&mut e.interval).range(0.01..=10.0).speed(0.005).prefix("every ").suffix(" mm")).changed();
                        changed |= ui.add(egui::DragValue::new(&mut e.min_power).range(0.0..=e.power).speed(0.5).prefix("min ").suffix(" %")).changed();
                        changed |= ui.add(egui::DragValue::new(&mut e.overscan).range(0.0..=50.0).speed(0.1).prefix("overscan ").suffix(" mm")).changed();
                        ui.horizontal(|ui| {
                            changed |= ui.checkbox(&mut e.bidirectional, "Both ways").changed();
                            changed |= ui.checkbox(&mut e.negative, "Negative").changed();
                        });
                        ui.end_row();
                    }
                    let dirty = dirty || e != *row;
                    if changed || dirty {
                        self.edits.insert(row.id, e);
                    }
                }
            });
            if let Some(row) = apply {
                self.apply_layer(run, &row);
            }
            ui.small("Line cuts along the paths; Fill engraves inside closed shapes (holes stay empty); Image engraves placed images. Settings save with the document; Edit › Undo takes them back.");
        });
        action
    }

    /// Typed, not yet applied settings of layer `id`.
    #[cfg(test)]
    pub fn edit(&mut self, row: LayerRow) {
        self.edits.insert(row.id, row);
    }
}

/// The screen rectangle of the bed (for its outline).
pub fn bed_rect(center: Pos2, scale: (f32, f32), size: (f64, f64), origin: Origin) -> Rect {
    Rect::from_two_pos(bed_to_screen(center, scale, size, origin, 0.0, 0.0), bed_to_screen(center, scale, size, origin, size.0, size.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 720, "height": 720})).unwrap();
        s.execute("shape.rectangle", &json!({"x": 72, "y": 72, "width": 72, "height": 72})).unwrap();
        s
    }

    fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Result<Value, String> + '_ {
        move |id, p| {
            s.check_params(id, &p).map_err(|e| e.to_string())?;
            s.execute(id, &p).map_err(|e| e.to_string())
        }
    }

    const BED: Bed = Bed { width_mm: 300.0, height_mm: 300.0, max_s: 1000, origin: Origin::BottomLeft, prologue: Vec::new(), epilogue: Vec::new() };

    #[test]
    fn bed_corners_land_where_each_origin_says() {
        let c = egui::pos2(100.0, 100.0);
        let k = (1.0, 1.0);
        let size = (200.0, 100.0);
        // The machine's 0, 0 is at its origin corner of the bed on screen (front edge at the bottom).
        assert_eq!(bed_to_screen(c, k, size, Origin::BottomLeft, 0.0, 0.0), egui::pos2(0.0, 150.0));
        assert_eq!(bed_to_screen(c, k, size, Origin::TopLeft, 0.0, 0.0), egui::pos2(0.0, 50.0));
        assert_eq!(bed_to_screen(c, k, size, Origin::TopRight, 0.0, 0.0), egui::pos2(200.0, 50.0));
        assert_eq!(bed_to_screen(c, k, size, Origin::BottomRight, 0.0, 0.0), egui::pos2(200.0, 150.0));
        // Each axis runs into the bed.
        assert_eq!(bed_to_screen(c, k, size, Origin::TopRight, 10.0, 10.0), egui::pos2(190.0, 60.0));
        for o in Origin::ALL {
            assert_eq!(bed_rect(c, k, size, o), Rect::from_min_max(egui::pos2(0.0, 50.0), egui::pos2(200.0, 150.0)));
        }
    }

    #[test]
    fn export_params_carry_the_machine() {
        let p = export_params(Some(&Bed { origin: Origin::TopRight, ..BED }), true, 0.1, Some("a.gcode"));
        assert_eq!(
            p,
            json!({"selectionOnly": true, "tolerance": 0.1, "bedWidth": 300.0, "bedHeight": 300.0, "maxS": 1000, "origin": "topRight", "prologue": [], "epilogue": [], "path": "a.gcode"})
        );
        assert!(export_params(None, false, 0.05, None).get("bedWidth").is_none());
    }

    #[test]
    fn without_a_machine_the_command_error_is_shown() {
        let mut s = session();
        let mut d = DocExport::default();
        assert!(d.generate(&mut runner(&mut s), None, None).is_none());
        assert!(d.error.as_deref().unwrap().contains("no machine bed size"), "{:?}", d.error);
    }

    #[test]
    fn generates_gcode_from_the_document() {
        let mut s = session();
        let mut d = DocExport::default();
        let g = d.generate(&mut runner(&mut s), Some(&BED), None).unwrap();
        assert!(g.gcode.starts_with("G21\nG90\n") && g.gcode.contains("M4 S200"));
        assert_eq!(g.contours, 1);
        assert!(d.error.is_none() && d.warnings.is_empty());
        // Art off a small bed: the command's error.
        assert!(d.generate(&mut runner(&mut s), Some(&Bed { width_mm: 30.0, height_mm: 30.0, ..BED }), None).is_none());
        assert!(d.error.as_deref().unwrap().contains("outside the bed"));
        // Type is warned about.
        s.execute("text.create", &json!({"x": 300, "y": 300, "text": "Hi"})).unwrap();
        d.generate(&mut runner(&mut s), Some(&BED), None).unwrap();
        assert!(d.warnings.iter().any(|w| w.contains("type")), "{:?}", d.warnings);
    }

    /// The exported file carries the machine's hooks, as the job the panel runs does.
    #[test]
    fn exported_file_has_the_machine_hooks() {
        let mut s = session();
        let mut d = DocExport::default();
        let bed = Bed { prologue: vec!["M8".into()], epilogue: vec!["M9".into()], ..BED };
        let path = std::env::temp_dir().join(format!("lasercraft-panel-air-{}.gcode", std::process::id()));
        let g = d.generate(&mut runner(&mut s), Some(&bed), path.to_str()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, g.gcode);
        assert!(text.starts_with("M8\nG21\n") && text.ends_with("M5\nM9\n"), "{text}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn exports_to_a_file() {
        let mut s = session();
        let mut d = DocExport::default();
        let path = std::env::temp_dir().join(format!("lasercraft-panel-{}.gcode", std::process::id()));
        let g = d.generate(&mut runner(&mut s), Some(&BED), path.to_str()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), g.gcode);
        assert!(d.status.contains("Wrote"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn layer_settings_are_read_and_set_through_commands() {
        let mut s = session();
        let mut d = DocExport::default();
        d.refresh_layers(&mut runner(&mut s));
        assert_eq!(d.layers.len(), 1);
        let row = LayerRow { speed: 600.0, power: 70.0, passes: 2, ..d.layers[0].clone() };
        d.edit(row.clone());
        d.apply_layer(&mut runner(&mut s), &row);
        assert!(d.error.is_none());
        assert_eq!((d.layers[0].speed, d.layers[0].power, d.layers[0].passes, d.layers[0].custom), (600.0, 70.0, 2, true));
        assert!(d.edits.is_empty());
        // Refused values show the command's error; the document keeps what it had.
        d.apply_layer(&mut runner(&mut s), &LayerRow { power: 150.0, ..row.clone() });
        assert!(d.error.is_some());
        assert_eq!(d.layers[0].power, 70.0);
        // One undo step.
        s.execute("edit.undo", &json!({})).unwrap();
        d.refresh_layers(&mut runner(&mut s));
        assert!(!d.layers[0].custom);
    }

    /// A layer set to Fill in the table engraves scan lines, and the table shows its fill settings.
    #[test]
    fn a_fill_layer_from_the_table() {
        let mut s = session();
        let mut d = DocExport::default();
        d.refresh_layers(&mut runner(&mut s));
        let row = LayerRow { mode: LaserMode::Fill, interval: 1.0, angle: 0.0, overscan: 1.0, crosshatch: true, ..d.layers[0].clone() };
        d.apply_layer(&mut runner(&mut s), &row);
        assert!(d.error.is_none(), "{:?}", d.error);
        let l = &d.layers[0];
        assert_eq!((l.mode, l.interval, l.overscan, l.crosshatch), (LaserMode::Fill, 1.0, 1.0, true));
        let g = d.generate(&mut runner(&mut s), Some(&BED), None).unwrap();
        // 25 lines each way across the 25.4 mm square.
        assert_eq!(g.contours, 50);
        assert!(g.gcode.contains("M4 S0"));
        // The dry run burns the scan lines only: the overscan runs at S0 show as travel.
        let sim = crate::laser_sim::simulate(&g.gcode, 1000.0);
        let on = sim.steps.iter().filter(|st| st.laser_on).count();
        assert_eq!(on, 50);
        assert!(sim.steps.len() > on);
        // The fill row is drawn.
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = d.ui(ui, &mut runner(&mut s), Some(&BED), "Test machine");
        });
        out.textures_delta.clear();
        let mut all = vec![];
        fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
        assert!(all.iter().any(|t| t == "Crosshatch") && all.iter().any(|t| t.contains("every")) && all.iter().any(|t| t == "1.000"), "{all:?}");
    }

    /// An Image layer from the table engraves a placed image.
    #[test]
    fn an_image_layer_from_the_table() {
        let mut s = session();
        // A 16 × 16 black-to-white image, 1 in square, placed through the engine.
        let img = image::RgbaImage::from_fn(16, 16, |x, _| {
            let v = (x * 17) as u8;
            image::Rgba([v, v, v, 255])
        });
        let path = std::env::temp_dir().join(format!("lasercraft-panel-image-{}.png", std::process::id()));
        img.save(&path).unwrap();
        s.execute("file.place", &json!({"path": path.to_str().unwrap(), "link": false, "rect": [300, 300, 72, 72]})).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut d = DocExport::default();
        d.refresh_layers(&mut runner(&mut s));
        let row = LayerRow { mode: LaserMode::Image, dither: Dither::Grayscale, interval: 1.0, min_power: 5.0, power: 50.0, ..d.layers[0].clone() };
        d.apply_layer(&mut runner(&mut s), &row);
        assert!(d.error.is_none(), "{:?}", d.error);
        assert_eq!((d.layers[0].mode, d.layers[0].dither, d.layers[0].min_power), (LaserMode::Image, Dither::Grayscale, 5.0));
        let g = d.generate(&mut runner(&mut s), Some(&BED), None).unwrap();
        // 25 rows across the 25.4 mm image; the rectangle on the same layer is left out.
        assert_eq!(g.contours, 25);
        assert!(d.warnings.iter().any(|w| w.contains("paths on Image layers")), "{:?}", d.warnings);
        assert!(g.gcode.contains("M4 S500"), "the darkest pixels at full power");
    }

    /// The section drawn headless: without a machine it says so; with one, Generate and the
    /// layer table are there, and nothing panics.
    #[test]
    fn section_draws_in_a_headless_frame() {
        let mut s = session();
        let ctx = egui::Context::default();
        let mut d = DocExport::default();
        for bed in [None, Some(BED)] {
            let mut action = None;
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 800.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                action = d.ui(ui, &mut runner(&mut s), bed.as_ref(), "Test machine");
            });
            out.textures_delta.clear();
            assert!(action.is_none());
            fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
                    _ => {}
                }
            }
            let mut all = vec![];
            for c in &out.shapes {
                texts(&c.shape, &mut all);
            }
            let text = all.join("\n");
            assert!(text.contains("Generate from document") && text.contains("Export G-code"), "{text}");
            assert_eq!(text.contains("No machine profile loaded"), bed.is_none(), "{text}");
            assert!(text.contains("Speed (mm/min)"), "{text}");
        }
        assert_eq!(d.layers.len(), 1);
    }
}
