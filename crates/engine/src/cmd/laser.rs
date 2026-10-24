//! LaserCraft: laser jobs made from the document (`lasercraft_cam`). `laser.exportGcode` writes
//! GRBL G-code for review (nothing is sent to a machine); `laser.setLayer` and `laser.layers` set
//! and read the per-layer cut settings, which save with the document.

use lasercraft_cam::machine::MAX_BED_MM;
use lasercraft_cam::{JobOptions, LaserMode, LayerSettings, Origin};
use lasercraft_core::{Machine, PlanError};
use serde_json::{Value, json};

use super::*;

const EXPORT: &str = "laser.exportGcode";
const SET: &str = "laser.setLayer";

const NO_BED: &str = "no machine bed size: select a machine profile or pass bedWidth/bedHeight";
/// Most hook lines (`prologue`, `epilogue`) and their longest line.
const MAX_HOOK_LINES: usize = 1024;
const MAX_HOOK_LINE: usize = 120;

/// The G-code lines `key` gives (a machine's air-assist and accessory hooks). Untrusted: only
/// comments, M-codes that don't switch the laser on (no M3/M4) and G4 dwells, so hooks can't move
/// the head or fire the laser.
fn hook_lines(p: &Value, key: &str) -> Result<Vec<String>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(vec![]) };
    let lines = v.as_array().ok_or_else(|| bad(EXPORT, format!("{key} must be a list of G-code lines")))?;
    if lines.len() > MAX_HOOK_LINES {
        return Err(bad(EXPORT, format!("{key} has more than {MAX_HOOK_LINES} lines")));
    }
    lines
        .iter()
        .map(|l| {
            let line = l.as_str().ok_or_else(|| bad(EXPORT, format!("{key} must be a list of G-code lines")))?.trim();
            if line.len() > MAX_HOOK_LINE || !line.is_ascii() || line.contains(['\n', '\r']) {
                return Err(bad(EXPORT, format!("{key}: `{line}` is not one ASCII line of at most {MAX_HOOK_LINE} characters")));
            }
            let code = line.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
            let mut words = code.split_whitespace();
            let allowed = match words.next() {
                None => true,
                Some(w) if w.starts_with('M') => w.get(1..).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| !matches!(n, 3 | 4)),
                Some(w) if w.starts_with('G') => {
                    w.get(1..).and_then(|n| n.parse::<u32>().ok()) == Some(4) && words.all(|a| a.starts_with('P') || a.starts_with('S'))
                }
                Some(_) => false,
            };
            if !allowed {
                return Err(bad(EXPORT, format!("{key}: `{line}` is not allowed (only comments, M-codes other than M3/M4, and G4 dwells)")));
            }
            Ok(line.to_string())
        })
        .collect()
}

/// The spindle value full power is (GRBL's `$30`).
const DEFAULT_MAX_S: u64 = 1000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "laser.exportGcode",
            "Export Laser G-code",
            [],
            None,
            "{bedWidth, bedHeight (mm, from the machine profile; art outside the bed is refused), origin?: the machine's corner: bottomLeft (default: x right, y up)|topLeft (x right, y down)|topRight (x left, y down)|bottomRight (x left, y up), maxS?: 1000 (GRBL $30: the S value of 100 % power), path?, selectionOnly?: false, tolerance?: 0.05 (mm curves are flattened to, 0.001 to 5), artboard?: 0 (the bed: the origin is its corner), prologue?: [G-code lines before the job], epilogue?: [lines after it] (the machine's air-assist and accessory hooks: comments, M-codes other than M3/M4, and G4 dwells only; at most 1024 lines of 120 characters)} → {gcode, warnings, contours, operations, path?, bytes?}. GRBL laser-mode G-code (M4 dynamic power, $32=1) of the visible, printable layers' paths, each layer cut with its laser.setLayer settings (bottom layer first, holes before the outlines around them, then nearest first). Review it before running it: nothing is sent to a machine",
            has_doc,
            export_gcode
        ),
        cmd!(
            "laser.setLayer",
            "Laser Layer Settings",
            [],
            None,
            "{layer: layer id, speed?: mm/min (default 1000), power?: 0–100 % (default 20), passes?: 1–100 (default 1), enabled?: true, mode?: line, clear?: false (back to the defaults)} → {layer, speed, power, passes, enabled, mode}. Values not given keep the layer's current ones. Saved with the document",
            has_doc,
            set_layer
        ),
        cmd!(
            query "laser.layers",
            "Laser Layers",
            [],
            None,
            "{} → {layers: [{layer, name, speed, power, passes, enabled, mode, custom}]}: every layer (sublayers too) with the laser settings it cuts with, top-level layers bottom first",
            has_doc,
            list_layers
        ),
    ]
}

fn plan_error(e: PlanError) -> String {
    match e {
        PlanError::OutOfBounds => "the art lies outside the bed: move it onto the artboard or give a larger bedWidth / bedHeight".into(),
        PlanError::InvalidParameter => "the job has a value out of range (bed size, speed, power or a coordinate)".into(),
    }
}

fn bed_side(p: &Value, key: &str) -> Result<f64> {
    let v = p.get(key).and_then(Value::as_f64).ok_or_else(|| bad(EXPORT, NO_BED))?;
    if !(v.is_finite() && v > 0.0 && v <= MAX_BED_MM) {
        return Err(bad(EXPORT, format!("{key} must be more than 0 and at most {MAX_BED_MM} mm, not {v}")));
    }
    Ok(v)
}

fn export_gcode(s: &mut Session, p: &Value) -> Result<Value> {
    if !(p.is_object() || p.is_null()) {
        return Err(bad(EXPORT, "params must be an object"));
    }
    check_numbers(p, &["tolerance", "artboard", "bedWidth", "bedHeight", "maxS"], EXPORT)?;
    let origin = match str_param(p, "origin") {
        None => Origin::BottomLeft,
        Some(o) => Origin::parse(o).ok_or_else(|| bad(EXPORT, format!("origin `{o}`: bottomLeft, topLeft, topRight or bottomRight")))?,
    };
    let artboard = match p.get("artboard") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(EXPORT, "artboard must be a whole number from 0"))?,
    };
    let max_s = match p.get("maxS") {
        None | Some(Value::Null) => DEFAULT_MAX_S,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=u64::from(lasercraft_cam::machine::MAX_S)).contains(n))
            .ok_or_else(|| bad(EXPORT, format!("maxS must be a whole number from 1 to {}", lasercraft_cam::machine::MAX_S)))?,
    };
    let machine = Machine { width_mm: bed_side(p, "bedWidth")?, height_mm: bed_side(p, "bedHeight")?, max_s: u32::try_from(max_s).unwrap_or(1000) };
    let selection = if bool_or(p, "selectionOnly", false) {
        let ids = edit::selected_roots(s)?;
        if ids.is_empty() {
            return Err(bad(EXPORT, "nothing selected"));
        }
        Some(ids)
    } else {
        None
    };
    let o =
        JobOptions { tolerance_mm: f64_or(p, "tolerance", JobOptions::default().tolerance_mm), origin, artboard, selection, ..JobOptions::default() };
    let out = lasercraft_cam::document_to_job(&s.doc()?.doc, &o).map_err(|e| bad(EXPORT, e))?;
    if out.job.operations.is_empty() {
        let why = if out.warnings.is_empty() { String::new() } else { format!(" ({})", out.warnings.join("; ")) };
        return Err(bad(EXPORT, format!("nothing to cut: no visible paths on enabled, printable layers{why}")));
    }
    let (prologue, epilogue) = (hook_lines(p, "prologue")?, hook_lines(p, "epilogue")?);
    let job = out.job.to_grbl(machine).map_err(|e| bad(EXPORT, plan_error(e)))?;
    let mut gcode = String::with_capacity(job.len());
    for l in &prologue {
        gcode.push_str(l);
        gcode.push('\n');
    }
    gcode.push_str(&job);
    for l in &epilogue {
        gcode.push_str(l);
        gcode.push('\n');
    }
    let mut r = json!({
        "gcode": gcode,
        "warnings": out.warnings,
        "contours": out.contours,
        "operations": out.job.operations.len(),
    });
    if let Some(path) = str_param(p, "path") {
        fileio::write_file(path, gcode.as_bytes())?;
        r["path"] = json!(path);
        r["bytes"] = json!(gcode.len());
    }
    Ok(r)
}

fn settings_json(id: NodeId, s: &LayerSettings) -> Value {
    json!({
        "layer": id.0,
        "speed": s.speed_mm_min,
        "power": s.power_percent,
        "passes": s.passes,
        "enabled": s.enabled,
        "mode": s.mode.id(),
    })
}

fn set_layer(s: &mut Session, p: &Value) -> Result<Value> {
    check_numbers(p, &["speed", "power", "passes"], SET)?;
    let layer = p.get("layer").ok_or_else(|| bad(SET, "missing `layer` (a layer id)"))?;
    let id = checked_id(s, layer, SET)?;
    let doc = &s.doc()?.doc;
    if !doc.node(id).is_some_and(|n| n.is_layer()) {
        return Err(bad(SET, format!("object {} is not a layer", id.0)));
    }
    let current = lasercraft_cam::layer_settings(doc).get(&id).copied();
    let new = if bool_or(p, "clear", false) {
        None
    } else {
        let mut v = current.unwrap_or_default();
        v.speed_mm_min = f64_or(p, "speed", v.speed_mm_min);
        v.power_percent = f64_or(p, "power", v.power_percent);
        if let Some(n) = p.get("passes").filter(|n| !n.is_null()) {
            v.passes = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(SET, "passes must be a whole number"))?;
        }
        match p.get("enabled") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(b)) => v.enabled = *b,
            Some(_) => return Err(bad(SET, "enabled must be true or false")),
        }
        if let Some(m) = str_param(p, "mode") {
            v.mode = LaserMode::parse(m).ok_or_else(|| bad(SET, format!("mode `{m}`: only line is available")))?;
        }
        v.validate().map_err(|e| bad(SET, e))?;
        Some(v)
    };
    if new != current {
        s.edit("Laser Layer Settings", |d, _| lasercraft_cam::set_layer_settings(d, id, new).map_err(|e| bad(SET, e)))?;
    }
    Ok(settings_json(id, &new.unwrap_or_default()))
}

fn list_layers(s: &mut Session, _: &Value) -> Result<Value> {
    let doc = &s.doc()?.doc;
    let stored = lasercraft_cam::layer_settings(doc);
    let mut out = vec![];
    // A sublayer without settings cuts with its parent's (see lasercraft_cam's walk).
    fn visit(n: &vectorcraft_doc::Node, inherited: LayerSettings, stored: &std::collections::BTreeMap<NodeId, LayerSettings>, out: &mut Vec<Value>) {
        if !n.is_layer() {
            return;
        }
        let own = stored.get(&n.id).copied();
        let s = own.unwrap_or(inherited);
        let mut v = settings_json(n.id, &s);
        v["name"] = json!(n.name.clone().unwrap_or_default());
        v["custom"] = json!(own.is_some());
        out.push(v);
        for c in n.children().into_iter().flatten() {
            visit(c, s, stored, out);
        }
    }
    for l in &doc.layers {
        visit(l, LayerSettings::default(), &stored, &mut out);
    }
    Ok(json!({ "layers": out }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        // 10 × 10 in.
        s.execute("file.new", &json!({"width": 720, "height": 720})).unwrap();
        s
    }

    /// `laser.exportGcode` with `p`, on a 300 × 300 mm bed unless `p` gives one.
    fn export(s: &mut Session, mut p: Value) -> Result<Value> {
        for key in ["bedWidth", "bedHeight"] {
            if p.get(key).is_none() {
                p[key] = json!(300);
            }
        }
        s.execute(EXPORT, &p)
    }

    fn layer(s: &Session) -> u64 {
        s.doc().unwrap().doc.layers[0].id.0
    }

    #[test]
    fn exports_gcode_from_the_document() {
        let mut s = session();
        s.execute("shape.rectangle", &json!({"x": 72, "y": 72, "width": 72, "height": 72})).unwrap();
        s.execute("shape.ellipse", &json!({"x": 288, "y": 288, "width": 144, "height": 144})).unwrap();
        let r = export(&mut s, json!({})).unwrap();
        let g = r["gcode"].as_str().unwrap();
        assert!(g.starts_with("G21\nG90\nM5\n"), "{g}");
        assert!(g.contains("M4 S200"), "default power 20 %");
        assert!(g.contains("F1000.0"));
        // The rectangle's corner, 1 in right and 8 in up.
        assert!(g.contains("X25.400 Y203.200") || g.contains("X50.800 Y203.200"), "{g}");
        assert_eq!(r["contours"], 2);
        assert!(r["warnings"].as_array().unwrap().is_empty());
        // Each corner of the bed counts from its own corner of the artboard.
        let t = export(&mut s, json!({"origin": "topLeft"})).unwrap();
        assert!(t["gcode"].as_str().unwrap().contains("Y25.400"));
        let t = export(&mut s, json!({"origin": "topRight"})).unwrap();
        assert!(t["gcode"].as_str().unwrap().contains("X203.200 Y25.400") || t["gcode"].as_str().unwrap().contains("X228.600 Y25.400"));
        let t = export(&mut s, json!({"origin": "bottomRight"})).unwrap();
        assert!(t["gcode"].as_str().unwrap().contains(" Y203.200"));
        assert!(export(&mut s, json!({"origin": "centre"})).is_err());
        assert!(export(&mut s, json!({"tolerance": "fine"})).is_err());
        assert!(export(&mut s, json!({"tolerance": 0})).is_err());
    }

    #[test]
    fn layer_settings_drive_the_cut_and_save_with_the_document() {
        let mut s = session();
        s.execute("shape.rectangle", &json!({"x": 72, "y": 72, "width": 72, "height": 72})).unwrap();
        let l = layer(&s);
        let r = s.execute(SET, &json!({"layer": l, "speed": 600, "power": 75, "passes": 2})).unwrap();
        assert_eq!(
            (r["speed"].as_f64(), r["power"].as_f64(), r["passes"].as_u64(), r["enabled"].as_bool()),
            (Some(600.0), Some(75.0), Some(2), Some(true))
        );
        let g = export(&mut s, json!({})).unwrap();
        assert_eq!(g["contours"], 2);
        assert!(g["gcode"].as_str().unwrap().contains("M4 S750"));
        assert!(g["gcode"].as_str().unwrap().contains("F600.0"));
        // Only what's given changes.
        let r = s.execute(SET, &json!({"layer": l, "power": 50})).unwrap();
        assert_eq!((r["speed"].as_f64(), r["power"].as_f64()), (Some(600.0), Some(50.0)));
        // It's one undoable step each.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("laser.layers", &json!({})).unwrap()["layers"][0]["power"], 75.0);
        // It saves and opens again with the document.
        let bytes = vectorcraft_format::save(&s.doc().unwrap().doc, false);
        let back = vectorcraft_format::load(&bytes).unwrap();
        assert_eq!(lasercraft_cam::layer_settings(&back).get(&NodeId(l)).map(|v| v.power_percent), Some(75.0));
        // Bad values and non-layers are refused.
        assert!(s.execute(SET, &json!({"layer": l, "power": 120})).is_err());
        assert!(s.execute(SET, &json!({"layer": l, "speed": 0})).is_err());
        assert!(s.execute(SET, &json!({"layer": l, "passes": 0})).is_err());
        assert!(s.execute(SET, &json!({"layer": l, "mode": "fill"})).is_err());
        assert!(s.execute(SET, &json!({"layer": 999_999})).is_err());
        let rect = s.doc().unwrap().doc.layers[0].children().unwrap()[0].id.0;
        assert!(s.execute(SET, &json!({"layer": rect, "power": 10})).is_err());
        // Disabled: nothing to cut.
        s.execute(SET, &json!({"layer": l, "enabled": false})).unwrap();
        assert!(export(&mut s, json!({})).is_err());
        // Clear: back to the defaults.
        let r = s.execute(SET, &json!({"layer": l, "clear": true})).unwrap();
        assert_eq!(r["power"], 20.0);
        assert_eq!(s.execute("laser.layers", &json!({})).unwrap()["layers"][0]["custom"], false);
    }

    #[test]
    fn hidden_layers_are_skipped_and_selection_limits_the_job() {
        let mut s = session();
        let a = s.execute("shape.rectangle", &json!({"x": 72, "y": 72, "width": 72, "height": 72})).unwrap()["id"].clone();
        s.execute("shape.rectangle", &json!({"x": 300, "y": 300, "width": 72, "height": 72})).unwrap();
        assert_eq!(export(&mut s, json!({})).unwrap()["contours"], 2);
        s.execute("select.set", &json!({"ids": [a]})).unwrap();
        assert_eq!(export(&mut s, json!({"selectionOnly": true})).unwrap()["contours"], 1);
        s.execute("select.none", &json!({})).unwrap();
        assert!(export(&mut s, json!({"selectionOnly": true})).is_err());
        // Hiding the layer leaves nothing.
        let l = layer(&s);
        s.edit("hide", |d, _| {
            d.node_mut(NodeId(l)).unwrap().visible = false;
            Ok(())
        })
        .unwrap();
        assert!(export(&mut s, json!({})).is_err());
    }

    #[test]
    fn machine_hooks_wrap_the_job() {
        let mut s = session();
        s.execute("shape.rectangle", &json!({"x": 72, "y": 72, "width": 72, "height": 72})).unwrap();
        let r = export(&mut s, json!({"prologue": ["; air assist", "M8", "G4 P0.500"], "epilogue": ["G4 P1.000", "M9"]})).unwrap();
        let g = r["gcode"].as_str().unwrap();
        assert!(g.starts_with("; air assist\nM8\nG4 P0.500\nG21\nG90\n"), "{g}");
        assert!(g.ends_with("M5\nG4 P1.000\nM9\n"), "{g}");
        // Hooks can't move the head or fire the laser.
        for bad in ["M3 S1000", "M4", "G1 X10", "G0 X0 Y0", "G4 X5", "$H", "M8\nG1 X5", "T1"] {
            assert!(export(&mut s, json!({"prologue": [bad]})).is_err(), "{bad}");
        }
        assert!(export(&mut s, json!({"epilogue": "M9"})).is_err());
        assert!(export(&mut s, json!({"epilogue": vec!["M9"; 1025]})).is_err());
        assert!(export(&mut s, json!({"prologue": ["M106 S255", "M7", ""]})).is_ok());
    }

    #[test]
    fn art_off_the_bed_is_refused() {
        let mut s = session();
        s.execute("shape.rectangle", &json!({"x": 648, "y": 72, "width": 36, "height": 36})).unwrap();
        let e = export(&mut s, json!({"bedWidth": 200, "bedHeight": 300})).unwrap_err().to_string();
        assert!(e.contains("outside the bed"), "{e}");
        assert!(export(&mut s, json!({"bedWidth": 300, "bedHeight": 300})).is_ok());
        assert!(export(&mut s, json!({"bedWidth": -1})).is_err());
        // The bed comes from the machine profile: no silent default.
        let e = s.execute(EXPORT, &json!({})).unwrap_err().to_string();
        assert!(e.contains("no machine bed size"), "{e}");
        assert!(s.execute(EXPORT, &json!({"bedWidth": 300})).is_err());
        assert!(export(&mut s, json!({"maxS": 0})).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writes_the_file() {
        let mut s = session();
        s.execute("shape.rectangle", &json!({"x": 72, "y": 72, "width": 72, "height": 72})).unwrap();
        let path = std::env::temp_dir().join(format!("lasercraft-{}.gcode", std::process::id()));
        let r = export(&mut s, json!({"path": path.to_str().unwrap()})).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(r["bytes"].as_u64(), Some(text.len() as u64));
        assert_eq!(r["gcode"].as_str(), Some(text.as_str()));
        let _ = std::fs::remove_file(&path);
    }
}
