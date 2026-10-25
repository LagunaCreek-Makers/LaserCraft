//! LaserCraft laser operations editor. Offline-only: no machine motion or firing.
use crate::accessories::{Accessory, AccessoryConfig, AccessoryMode, AccessoryTransport, send_independent};
use crate::accessory_lifecycle::{Coordinator, Stage};
use crate::accessory_macros::{MacroAction, MacroConfig, MacroEvent, MacroRule};
use crate::air_assist::{AirConfig, AirMode};
use crate::grbl_monitor::{self, Event, GrblStatus, Link, Monitor};
use crate::grbl_stream::Progress;
use crate::laser_doc::{self, Bed, DocExport};
use crate::laser_sim;
use crate::laser_tests::{self, TestKind, TestResult, TestSettings};
use crate::macro_routines::{Context, EventBinding, Routine, RoutineDatabase, Step};
use crate::ruida_monitor;
use crate::ruida_protocol;
use egui::{self, Color32};
use lasercraft_cam::Origin;
use lasercraft_cam::hooks::job_hooks;
use lasercraft_core::{Job, Machine, Operation, Point};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A cut preset: the speed, power, passes, interval and air assist the
/// calibration tests, the fill preview and the diagnostic square use (and material presets
/// save). Layer modes for the document are set in its layer table, not here.
struct LaserLayer {
    name: String,
    color: Color32,
    enabled: bool,
    speed: f64,
    power: f64,
    passes: u32,
    interval: f64,
    air: AirMode,
}
impl LaserLayer {
    fn new(name: &str, color: Color32) -> Self {
        Self { name: name.into(), color, enabled: true, speed: 1200.0, power: 35.0, passes: 1, interval: 0.10, air: AirMode::Inherit }
    }
}
pub struct LaserPanel {
    pub open: bool,
    layers: Vec<LaserLayer>,
    selected: usize,
    machine_width: f64,
    machine_height: f64,
    max_s: u32,
    /// The machine's fastest move (mm/min), for time estimates (its profile's `max_rate`).
    max_rate: f64,
    /// The corner the machine counts from (saved in its profile as `origin`).
    machine_origin: Origin,
    /// The name of the machine profile loaded (Load machine, or saved): jobs from the document
    /// are made for its bed only.
    loaded_profile: Option<String>,
    /// The job made from the document and its layers' settings.
    doc_export: DocExport,
    message: String,
    generated_gcode: String,
    preview: Option<Job>,
    profiles: Vec<Value>,
    materials: Vec<Value>,
    active_profile: usize,
    active_material: usize,
    connection_port: String,
    connection_baud: u32,
    controller: String,
    monitor: Option<Monitor>,
    monitor_status: GrblStatus,
    monitor_log: Vec<String>,
    detected_ports: Vec<String>,
    jog_distance: f64,
    jog_feed: f64,
    motion_unlocked: bool,
    stream_progress: Option<Progress>,
    stream_armed: bool,
    ruida_listener: Option<ruida_monitor::Listener>,
    ruida_port: u16,
    ruida_log: Vec<String>,
    ruida_inventory: ruida_monitor::Inventory,
    ruida_hex_input: String,
    ruida_inspection: String,
    sim_step: usize,
    sim_source: String,
    sim_playing: bool,
    sim_time: f64,
    sim_rate: f64,
    sim_zoom: f32,
    sim_pan: egui::Vec2,
    exposure_resolution: usize,
    exposure_grayscale: bool,
    acceleration: f64,
    test_settings: TestSettings,
    test_notes: String,
    test_results: Vec<TestResult>,
    test_status: String,
    air_config: AirConfig,
    accessory_config: AccessoryConfig,
    coordinator: Coordinator,
    macros: MacroConfig,
    macro_log: Vec<String>,
    macro_text: String,
    macro_text_message: String,
    routine_db: RoutineDatabase,
    routine_text: String,
    routine_message: String,
    last_macro_stage: Stage,
    canvas_drag: Option<crate::macro_canvas::DragState>,
    canvas_editors: Vec<crate::macro_canvas::CanvasEditorState>,
    debugger: crate::macro_debugger::Debugger,
}
impl Default for LaserPanel {
    fn default() -> Self {
        Self {
            open: true,
            layers: vec![LaserLayer::new("C00", Color32::BLACK), LaserLayer::new("C01", Color32::RED)],
            selected: 0,
            machine_width: 300.0,
            machine_height: 200.0,
            max_s: 1000,
            max_rate: 6000.0,
            machine_origin: Origin::BottomLeft,
            loaded_profile: None,
            doc_export: DocExport::default(),
            message: "Offline mode — no laser is connected".into(),
            generated_gcode: String::new(),
            preview: None,
            profiles: vec![],
            materials: vec![],
            active_profile: 0,
            active_material: 0,
            connection_port: String::new(),
            connection_baud: 115200,
            controller: "GRBL".into(),
            monitor: None,
            monitor_status: GrblStatus::default(),
            monitor_log: Vec::new(),
            detected_ports: Vec::new(),
            jog_distance: 1.0,
            jog_feed: 300.0,
            motion_unlocked: false,
            stream_progress: None,
            stream_armed: false,
            ruida_listener: None,
            ruida_port: 50200,
            ruida_log: Vec::new(),
            ruida_inventory: ruida_monitor::Inventory::default(),
            ruida_hex_input: String::new(),
            ruida_inspection: String::new(),
            sim_step: 0,
            sim_source: String::new(),
            sim_playing: false,
            sim_time: 0.0,
            sim_rate: 1.0,
            sim_zoom: 1.0,
            sim_pan: egui::Vec2::ZERO,
            exposure_resolution: 100,
            exposure_grayscale: false,
            acceleration: 500.0,
            test_settings: TestSettings::default(),
            test_notes: String::new(),
            test_results: Vec::new(),
            test_status: String::new(),
            air_config: AirConfig::default(),
            accessory_config: AccessoryConfig::default(),
            coordinator: Coordinator::default(),
            macros: MacroConfig::default(),
            macro_log: Vec::new(),
            macro_text: String::new(),
            macro_text_message: String::new(),
            routine_db: RoutineDatabase::default(),
            routine_text: String::new(),
            routine_message: String::new(),
            last_macro_stage: Stage::Idle,
            canvas_drag: None,
            canvas_editors: Vec::new(),
            debugger: Default::default(),
        }
        .load_profiles()
    }
}
impl LaserPanel {
    fn settings_path() -> Option<PathBuf> {
        lasercraft_cam::default_profiles_path()
    }
    fn load_profiles(self) -> Self {
        let data = Self::settings_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        self.load_profiles_from(&data)
    }
    /// The machines, materials and calibration results of `data` (`profiles.json`, or null when
    /// there is none), and the machine last loaded loaded again (`active_machine`), so jobs from
    /// the document are made for it from the start.
    fn load_profiles_from(mut self, data: &Value) -> Self {
        if data.is_object() {
            if let Some(p) = data["machines"].as_array() {
                self.profiles = p.clone();
            }
            if let Some(m) = data["materials"].as_array() {
                self.materials = m.clone();
            }
            if let Ok(results) = serde_json::from_value::<Vec<TestResult>>(data["calibration_results"].clone()) {
                self.test_results = results;
            }
            // Machine-level air-assist settings are applied when the machine profile is loaded.
        }
        if self.profiles.is_empty() {
            self.profiles.push(
                json!({"name":"GRBL 300 x 200", "controller":"GRBL", "width_mm":300.0,"height_mm":200.0,"max_s":1000,"port":"", "baud":115200}),
            );
        }
        if self.materials.is_empty() {
            self.materials.push(json!({"name":"Example plywood (UNVERIFIED)","speed":1200.0,"power":35.0,"passes":1,"interval":0.10}));
        }
        if let Some(name) = data["active_machine"].as_str() {
            let found = self.profiles.iter().position(|p| p["name"].as_str() == Some(name));
            match found.map(|i| (i, lasercraft_cam::MachineProfile::from_json(&self.profiles[i]))) {
                Some((i, Ok(_))) => {
                    self.active_profile = i;
                    self.apply_machine();
                }
                // Left unloaded: the document section asks for a machine.
                Some((_, Err(e))) => self.message = format!("The last machine profile could not be loaded: {e}"),
                None => self.message = format!("The last machine profile `{name}` is no longer saved: load a machine"),
            }
        }
        self
    }
    fn save_profiles(&self) -> Result<(), String> {
        let path = Self::settings_path().ok_or("Cannot determine configuration directory")?;
        std::fs::create_dir_all(path.parent().ok_or("Invalid path")?).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(
            &json!({"schema_version":1,"machines":self.profiles,"materials":self.materials,"calibration_results":self.test_results,"active_machine":self.loaded_profile}),
        )
        .map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }
    fn apply_machine(&mut self) {
        if let Some(p) = self.profiles.get(self.active_profile) {
            self.machine_width = p["width_mm"].as_f64().unwrap_or(300.0).clamp(1.0, 10000.0);
            self.machine_height = p["height_mm"].as_f64().unwrap_or(200.0).clamp(1.0, 10000.0);
            self.max_s = p["max_s"].as_u64().unwrap_or(1000).clamp(1, 100000) as u32;
            self.max_rate = p["max_rate"].as_f64().filter(|v| v.is_finite()).unwrap_or(6000.0).clamp(60.0, 200_000.0);
            self.acceleration = p["acceleration"].as_f64().filter(|v| v.is_finite()).unwrap_or(500.0).clamp(10.0, 10_000.0);
            self.controller = p["controller"].as_str().unwrap_or("GRBL").to_owned();
            // Profiles saved before `origin` existed get their controller's usual corner.
            self.machine_origin = p["origin"].as_str().and_then(Origin::parse).unwrap_or_else(|| Origin::default_for(&self.controller));
            self.loaded_profile = Some(p["name"].as_str().unwrap_or("Unnamed").to_owned());
            self.connection_port = p["port"].as_str().unwrap_or("").to_owned();
            self.connection_baud = p["baud"].as_u64().unwrap_or(115200).clamp(1200, 1000000) as u32;
            self.air_config = serde_json::from_value(p["air_assist"].clone()).unwrap_or_default();
            self.accessory_config = serde_json::from_value(p["accessories"].clone()).unwrap_or_default();
            self.macros = serde_json::from_value(p["macros"].clone()).unwrap_or_default();
            self.routine_db = serde_json::from_value(p["routines"].clone()).unwrap_or_default();
        }
    }
    fn run_macro_event(&mut self, event: MacroEvent, input: &str, layer: &str) {
        match self.macros.plan(event, input, layer, &self.accessory_config) {
            Ok(actions) => {
                if actions.is_empty() {
                    return;
                }
                self.macro_log.push(format!("Queued {} actions for {:?} (command acknowledgement is not output feedback)", actions.len(), event));
                std::thread::spawn(move || {
                    for action in actions {
                        if let Err(err) = send_independent(&action.device, action.activate) {
                            eprintln!("LaserCraft macro {} failed: {}", action.description, err);
                            break;
                        }
                    }
                });
            }
            Err(err) => self.macro_log.push(format!("Macro blocked: {err}")),
        }
    }
    fn apply_material(&mut self) {
        if let (Some(p), Some(l)) = (self.materials.get(self.active_material), self.layers.get_mut(self.selected)) {
            l.speed = p["speed"].as_f64().unwrap_or(l.speed).clamp(1.0, 30000.0);
            l.power = p["power"].as_f64().unwrap_or(l.power).clamp(0.0, 100.0);
            l.passes = p["passes"].as_u64().unwrap_or(l.passes as u64).clamp(1, 100) as u32;
            l.interval = p["interval"].as_f64().unwrap_or(l.interval).clamp(0.025, 1.0);
            l.air = serde_json::from_value(p["air_assist"].clone()).unwrap_or_default();
        }
    }
    /// The loaded machine's bed, which jobs from the document are made for, with its air-assist
    /// and accessory G-code (`Ok(None)`: no machine profile loaded yet, so none is guessed; an
    /// error when the air-assist or accessory settings are refused).
    fn bed(&self) -> Result<Option<Bed>, String> {
        if self.loaded_profile.is_none() {
            return Ok(None);
        }
        let (prologue, epilogue) = job_hooks(&self.air_config, AirMode::Inherit, &self.accessory_config)?;
        Ok(Some(Bed {
            max_rate: self.max_rate,
            acceleration: self.acceleration,
            width_mm: self.machine_width,
            height_mm: self.machine_height,
            max_s: self.max_s,
            origin: self.machine_origin,
            prologue,
            epilogue,
        }))
    }
    /// The document section, and what it asks for: a job from the document (wrapped in the
    /// machine's air-assist and accessory G-code by the command) goes to the dry run, and Export
    /// G-code asks where to save.
    fn document_section(&mut self, ui: &mut egui::Ui, run: &mut laser_doc::Run) {
        let bed = match self.bed() {
            Ok(bed) => bed,
            Err(e) => {
                ui.colored_label(vectorcraft_ui_egui::theme::Tokens::get(ui.ctx()).error, format!("Air-assist or accessory settings: {e}"));
                return;
            }
        };
        let name = self.loaded_profile.clone().unwrap_or_default();
        match self.doc_export.ui(ui, run, bed.as_ref(), &name) {
            Some(laser_doc::Action::Generated(g)) => self.take_document_job(g.gcode),
            Some(laser_doc::Action::ChooseExportPath) => {
                if bed.is_none() {
                    // No machine: the command says so (nothing to save).
                    let _ = self.doc_export.generate(run, None, None);
                    return;
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = rfd::FileDialog::new().add_filter("G-code", &["gcode", "nc", "gc"]).set_file_name("job.gcode").save_file()
                    && let Some(path) = path.to_str()
                    && let Some(g) = self.doc_export.generate(run, bed.as_ref(), Some(path))
                {
                    self.take_document_job(g.gcode);
                }
            }
            None => {}
        }
    }
    /// A job from the document (already wrapped in the machine's hooks) goes to the dry run.
    fn take_document_job(&mut self, gcode: String) {
        self.preview = None;
        self.generated_gcode = gcode.clone();
        self.sim_source = gcode;
        self.sim_step = 0;
        self.sim_time = 0.0;
        self.sim_playing = false;
        self.message = "Job made from the document; inspect it in the dry run. Not sent to the controller.".into();
    }
    /// Takes what the machine's monitor reported since last time (status, lines, job progress, a
    /// disconnect).
    fn drain_monitor(&mut self) {
        let mut disconnected = false;
        if let Some(monitor) = &self.monitor {
            for event in monitor.events.try_iter().take(128) {
                match event {
                    Event::Connected => self.message = "GRBL serial monitor connected (experimental motion controls)".into(),
                    Event::Disconnected(reason) => {
                        self.message = reason;
                        disconnected = true;
                    }
                    Event::Line(line) => {
                        if line.starts_with("ALARM:") {
                            self.monitor_status.alarm = Some(line.clone());
                        }
                        self.monitor_log.push(line);
                    }
                    Event::Status(status) => self.monitor_status = status,
                    Event::Ports(ports) => self.detected_ports = ports,
                    Event::JobProgress(progress) => {
                        if progress.failed.is_some() || progress.finished {
                            self.stream_armed = false;
                        }
                        self.stream_progress = Some(progress);
                    }
                }
            }
        }
        if disconnected {
            self.monitor = None;
            self.coordinator.shutdown();
        }
    }

    /// A machine request from automation (the control channel; MCP has no machine tools). The
    /// rule (Paul's: machine motion is panel-only): on a real machine, automation may only
    /// connect, disconnect, read status and settings, hold and stop; everything that moves the
    /// machine or fires the laser (resume, jog, home, unlock, start) is refused. On the built-in
    /// simulator, automation may drive everything. Which one is connected is decided when it is
    /// opened, never by a request.
    pub fn automation(&mut self, method: &str, p: &Value) -> Result<Value, String> {
        const PANEL_ONLY: &str =
            "machine motion is panel-only: on a real machine, automation may only connect, disconnect, read status and settings, hold and stop";
        self.drain_monitor();
        let Some(name) = method.strip_prefix("laser.machine.") else { return Err(format!("unknown machine request `{method}`")) };
        let moves = matches!(name, "resume" | "jog" | "home" | "unlock" | "start");
        match name {
            "status" => Ok(self.machine_status_json()),
            "connect" => {
                if self.monitor.is_some() {
                    return Err("a machine is already connected: disconnect first".into());
                }
                let expect =
                    lasercraft_machine::grbl::Expect { max_s: Some(f64::from(self.max_s)), bed: Some((self.machine_width, self.machine_height)) };
                if p["simulator"].as_bool() == Some(true) {
                    self.monitor = Some(Monitor::connect_simulator(expect));
                } else {
                    let port = p["port"].as_str().map(str::to_owned).unwrap_or_else(|| self.connection_port.clone());
                    if port.trim().is_empty() {
                        return Err("no serial port: pass `port`, or `simulator: true`".into());
                    }
                    let baud = p["baud"]
                        .as_u64()
                        .and_then(|b| u32::try_from(b).ok())
                        .filter(|b| (1200..=1_000_000).contains(b))
                        .unwrap_or(self.connection_baud);
                    self.monitor = Some(Monitor::connect(port, baud, expect));
                }
                Ok(self.machine_status_json())
            }
            _ => {
                let Some(m) = &self.monitor else { return Err("no machine is connected".into()) };
                if moves && m.link() != Link::Simulator {
                    return Err(PANEL_ONLY.into());
                }
                match name {
                    "disconnect" => m.disconnect(),
                    "readSettings" => m.settings(),
                    "hold" => m.hold(),
                    "stop" => m.stop(),
                    "resume" => m.resume(),
                    "home" => m.home(),
                    "unlock" => m.unlock(),
                    "jog" => {
                        let axis = p["axis"].as_str().and_then(|a| a.chars().next()).ok_or("jog needs `axis` (x, y or z)")?;
                        let distance = p["distance"].as_f64().ok_or("jog needs `distance` (mm)")?;
                        m.jog(axis, distance, p["feed"].as_f64().unwrap_or(600.0))?;
                    }
                    "start" => {
                        let code = p["gcode"].as_str().map(str::to_owned).unwrap_or_else(|| self.generated_gcode.clone());
                        m.start_job(code)?;
                    }
                    other => return Err(format!("unknown machine request `laser.machine.{other}`")),
                }
                Ok(json!({ "sent": name, "link": m.link().id() }))
            }
        }
    }

    fn machine_status_json(&self) -> Value {
        let s = &self.monitor_status;
        json!({
            "connected": self.monitor.is_some(),
            "link": self.monitor.as_ref().map(|m| m.link().id()),
            "state": s.state,
            "position": s.machine_position,
            "workPosition": s.work_position,
            "alarm": s.alarm,
            "job": self.stream_progress.as_ref().map(|p| json!({
                "sent": p.sent, "acknowledged": p.acknowledged, "total": p.total, "failed": p.failed, "finished": p.finished,
            })),
            "log": self.monitor_log.iter().rev().take(20).rev().collect::<Vec<_>>(),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, run: &mut laser_doc::Run) {
        if !self.open {
            return;
        }
        self.drain_monitor();
        if let Some(result) = self.coordinator.poll() {
            match result {
                Ok(()) if self.coordinator.stage == Stage::Ready => {
                    if let Some(code) = self.coordinator.pending_gcode.take() {
                        if let Some(m) = &self.monitor {
                            match m.start_job(code) {
                                Ok(()) => {
                                    self.coordinator.mark_running();
                                    self.stream_armed = false;
                                    self.message = "Job queued after accessory startup".into();
                                }
                                Err(e) => {
                                    self.message = e;
                                    self.coordinator.shutdown();
                                }
                            }
                        } else {
                            self.message = "Laser disconnected during accessory startup".into();
                            self.coordinator.shutdown();
                        }
                    }
                }
                Err(e) => self.message = e,
                _ => {}
            }
        }
        if self.coordinator.stage == Stage::Running
            && (self.stream_progress.as_ref().is_some_and(|p| p.failed.is_some()) || self.monitor_status.alarm.is_some())
        {
            self.coordinator.shutdown();
        }
        if self.coordinator.stage == Stage::Running
            && self.stream_progress.as_ref().is_some_and(|p| p.finished)
            && self.monitor_status.state.eq_ignore_ascii_case("Idle")
        {
            self.coordinator.shutdown();
        }
        // Event edges are observed after the GRBL coordinator updates. Dispatch never blocks the UI.
        let stage = self.coordinator.stage;
        if stage != self.last_macro_stage {
            let event = match stage {
                Stage::Running => Some(MacroEvent::JobStarted),
                Stage::Idle
                    if self.last_macro_stage == Stage::Stopping
                        && self.stream_progress.as_ref().is_some_and(|p| p.finished && p.failed.is_none()) =>
                {
                    Some(MacroEvent::JobCompleted)
                }
                Stage::Idle if self.last_macro_stage == Stage::Stopping => Some(MacroEvent::JobCancelled),
                Stage::Fault => Some(MacroEvent::JobFault),
                _ => None,
            };
            if let Some(event) = event {
                self.run_macro_event(event, "", "");
            }
            self.last_macro_stage = stage;
        }
        if self.monitor_log.len() > 100 {
            let excess = self.monitor_log.len() - 100;
            self.monitor_log.drain(..excess);
        }
        if let Some(listener) = &self.ruida_listener {
            for event in listener.events.try_iter().take(64) {
                let line = match event {
                    ruida_monitor::Event::Listening(p) => format!("Listening for UDP datagrams on port {p}"),
                    ruida_monitor::Event::Packet(p) => {
                        let line = format!("{} | {} bytes | {}", p.source, p.bytes, p.prefix_hex);
                        self.ruida_inventory.record(p);
                        line
                    }
                    ruida_monitor::Event::Error(e) => format!("Listener error: {e}"),
                    ruida_monitor::Event::Stopped => "Listener stopped".to_string(),
                };
                self.ruida_log.push(line);
            }
        }
        if self.ruida_log.len() > 100 {
            let excess = self.ruida_log.len() - 100;
            self.ruida_log.drain(..excess);
        }
        let mut open = self.open;
        egui::Window::new("LaserCraft | Laser Operations").open(&mut open).default_width(350.0).resizable(true).show(ctx,|ui|{
   egui::CollapsingHeader::new("Document → laser job").default_open(true).show(ui,|ui|self.document_section(ui,run));
   ui.separator();
   ui.horizontal(|ui| {ui.heading("Cut presets"); if ui.button("+ Add").clicked(){let idx=self.layers.len();self.layers.push(LaserLayer::new(&format!("C{idx:02}"),Color32::LIGHT_BLUE));self.selected=idx;}});
   egui::ScrollArea::vertical().max_height(190.0).show(ui,|ui|{
    for (i,layer) in self.layers.iter_mut().enumerate(){ui.horizontal(|ui|{ui.checkbox(&mut layer.enabled,"");let (rect,_)=ui.allocate_exact_size(egui::vec2(16.0,16.0),egui::Sense::hover());ui.painter().rect_filled(rect,2.0,layer.color);if ui.selectable_label(self.selected==i,&layer.name).clicked(){self.selected=i;}});}
   });
   ui.separator();
   if self.selected<self.layers.len(){
    let layer=&mut self.layers[self.selected];
    ui.horizontal(|ui|{ui.label("Layer");ui.text_edit_singleline(&mut layer.name);});
    ui.small("For calibration tests, the fill preview and diagnostics. The document's layers get their Line, Fill or Image mode in Document → laser job.");
    ui.add(egui::Slider::new(&mut layer.speed,1.0..=30000.0).text("Speed (mm/min)"));
    ui.add(egui::Slider::new(&mut layer.power,0.0..=100.0).text("Power (%)"));
    ui.add(egui::Slider::new(&mut layer.passes,1..=100).text("Passes"));
    ui.add(egui::Slider::new(&mut layer.interval,0.025..=1.0).text("Line interval (mm), fill preview"));
    egui::ComboBox::from_id_salt("layer_air_assist").selected_text(format!("Air assist: {}",layer.air.label())).show_ui(ui,|ui|{for v in [AirMode::Inherit,AirMode::On,AirMode::Off]{ui.selectable_value(&mut layer.air,v,v.label());}});
    if ui.button("Remove selected layer").clicked() && self.layers.len()>1 {self.layers.remove(self.selected);self.selected=self.selected.min(self.layers.len()-1);}
   }
   ui.separator();
   ui.collapsing("Phase 7 | Saved machines and materials",|ui|{
    ui.small("Profiles are stored locally as JSON. Presets are examples, NOT tested cutting recommendations.");
    ui.horizontal(|ui|{
     ui.label("Machine");
     egui::ComboBox::from_id_salt("lasercraft_machine").selected_text(self.profiles.get(self.active_profile).and_then(|p|p["name"].as_str()).unwrap_or("Select machine")).show_ui(ui,|ui|{
      for (i,p) in self.profiles.iter().enumerate(){ui.selectable_value(&mut self.active_profile,i,p["name"].as_str().unwrap_or("Unnamed"));}
     });
     if ui.button("Load machine").clicked(){
      self.apply_machine();
      // Remembered: the next start loads it again.
      if let Err(e)=self.save_profiles(){self.message=format!("Machine loaded, but not remembered: {e}");}
     }
    });
    ui.horizontal(|ui|{
     ui.label("Controller");
     egui::ComboBox::from_id_salt("lasercraft_controller").selected_text(&self.controller).show_ui(ui,|ui|{
      ui.selectable_value(&mut self.controller,"GRBL".to_owned(),"GRBL");
      ui.selectable_value(&mut self.controller,"Ruida".to_owned(),"Ruida (profile only)");
     });
    });
    ui.horizontal(|ui|{ui.label("Port / address");ui.text_edit_singleline(&mut self.connection_port);});
    ui.horizontal(|ui|{ui.label("Baud (GRBL)");ui.add(egui::DragValue::new(&mut self.connection_baud).range(1200..=1000000));});
    if ui.button("Save current machine as new profile").clicked(){
     self.profiles.push(json!({"name":format!("{} machine {}",self.controller,self.profiles.len()+1),"controller":self.controller,"width_mm":self.machine_width,"height_mm":self.machine_height,"max_s":self.max_s,"max_rate":self.max_rate,"acceleration":self.acceleration,"origin":self.machine_origin.id(),"port":self.connection_port,"baud":self.connection_baud,"air_assist":self.air_config,"accessories":self.accessory_config}));
     self.active_profile=self.profiles.len()-1;
     self.loaded_profile=self.profiles.last().and_then(|p|p["name"].as_str()).map(str::to_owned);
     self.message=match self.save_profiles(){Ok(())=>"Machine profile saved".into(),Err(e)=>format!("Save failed: {e}")};
    }
    ui.horizontal(|ui|{
     ui.label("Material");
     egui::ComboBox::from_id_salt("lasercraft_material").selected_text(self.materials.get(self.active_material).and_then(|m|m["name"].as_str()).unwrap_or("Select material")).show_ui(ui,|ui|{
      for (i,m) in self.materials.iter().enumerate(){ui.selectable_value(&mut self.active_material,i,m["name"].as_str().unwrap_or("Unnamed"));}
     });
     if ui.button("Apply to layer").clicked(){self.apply_material();}
    });
    if ui.button("Save selected layer as material preset").clicked()
     && let Some(l)=self.layers.get(self.selected){self.materials.push(json!({"name":format!("Custom material {}",self.materials.len()+1),"speed":l.speed,"power":l.power,"passes":l.passes,"interval":l.interval,"air_assist":l.air}));
      self.active_material=self.materials.len()-1;
      self.message=match self.save_profiles(){Ok(())=>"Material preset saved".into(),Err(e)=>format!("Save failed: {e}")};}
    ui.colored_label(Color32::YELLOW,"GRBL monitor and experimental supervised jogging are available below. Laser firing and job streaming are disabled.");
   });
   ui.collapsing("Phase 28 | Air-assist control",|ui|{
    ui.small("Commands are inserted into generated GRBL G-code; no direct GPIO, Modbus, or Ruida commands are sent. Verify output and solenoid wiring.");
    ui.checkbox(&mut self.air_config.enabled,"Enable automatic air assist");
    ui.checkbox(&mut self.air_config.default_on,"Default ON for inherited layers");
    ui.horizontal(|ui|{ui.label("ON G-code");ui.text_edit_singleline(&mut self.air_config.on_command);});
    ui.horizontal(|ui|{ui.label("OFF G-code");ui.text_edit_singleline(&mut self.air_config.off_command);});
    ui.horizontal(|ui|{ui.label("Preflow ms");ui.add(egui::DragValue::new(&mut self.air_config.preflow_ms).range(0..=30000));ui.label("Postflow ms");ui.add(egui::DragValue::new(&mut self.air_config.postflow_ms).range(0..=30000));});
    if let Err(e)=self.air_config.validate(){ui.colored_label(Color32::RED,e);}
    if ui.button("Save air-assist settings to selected machine").clicked(){
     if let Some(p)=self.profiles.get_mut(self.active_profile){p["air_assist"]=json!(self.air_config);}
     self.message=match self.save_profiles(){Ok(())=>"Machine air-assist settings saved".into(),Err(e)=>e};
    }
    ui.small("Material presets and layers may override the machine default. M8/M9 only work when configured on your controller.");
   });
   ui.separator();
   ui.collapsing("Phase 31 | Up to 256 auxiliary outputs / independent transports",|ui|{
    ui.colored_label(Color32::YELLOW,"For non-safety-critical accessories only. NEVER use G-code for CO2 fire suppression. M3/M4/M5 are reserved for laser control.");
    ui.small("M7/M8/M9 are controller-specific coolant outputs. PWM (e.g. M7 S128) works ONLY if your firmware explicitly implements it. M9 may disable several coolant outputs together.");
    if ui.button("+ Add accessory").clicked() && self.accessory_config.outputs.len()<256 {self.accessory_config.outputs.push(Accessory::default());}
    ui.label(format!("Configured devices: {} / 256",self.accessory_config.outputs.len()));
    let mut remove=None;
    for (i,a) in self.accessory_config.outputs.iter_mut().enumerate(){
     ui.collapsing(format!("Device {}: {}",i+1,a.name),|ui|{
      ui.horizontal(|ui|{ui.checkbox(&mut a.enabled,"Enabled");ui.text_edit_singleline(&mut a.name);if ui.button("Remove").clicked(){remove=Some(i);}});
      ui.horizontal(|ui|{ui.label("Transport");
       ui.selectable_value(&mut a.endpoint.transport,AccessoryTransport::Gcode,"Laser G-code");
       ui.selectable_value(&mut a.endpoint.transport,AccessoryTransport::Serial,"Separate COM/serial");
       ui.selectable_value(&mut a.endpoint.transport,AccessoryTransport::ModbusTcp,"Modbus TCP");
       ui.selectable_value(&mut a.endpoint.transport,AccessoryTransport::Mqtt,"MQTT");});
      if a.endpoint.transport!=AccessoryTransport::Gcode {
       ui.horizontal(|ui|{ui.label("Endpoint");ui.text_edit_singleline(&mut a.endpoint.address);});
       match a.endpoint.transport {
        AccessoryTransport::Serial => {ui.horizontal(|ui|{ui.label("Baud");ui.add(egui::DragValue::new(&mut a.endpoint.baud).range(1200..=3000000));});},
        AccessoryTransport::ModbusTcp => {ui.horizontal(|ui|{ui.label("Unit ID");ui.add(egui::DragValue::new(&mut a.endpoint.unit_id));ui.label("Coil address (zero-based)");ui.add(egui::DragValue::new(&mut a.endpoint.channel));});},
        AccessoryTransport::Mqtt => {ui.horizontal(|ui|{ui.label("Topic");ui.text_edit_singleline(&mut a.endpoint.mqtt_topic);});ui.horizontal(|ui|{ui.label("ON payload");ui.text_edit_singleline(&mut a.endpoint.mqtt_on);ui.label("OFF payload");ui.text_edit_singleline(&mut a.endpoint.mqtt_off);});if a.mode==AccessoryMode::Pwm {ui.horizontal(|ui|{ui.label("PWM payload template");ui.text_edit_singleline(&mut a.endpoint.mqtt_pwm_template);});}},
        _=>{}
       }
       ui.small("Independent accessory: manual test only. Not inserted into laser G-code. Never use for safety systems.");
       if ui.button("Send ON / PWM (manual)").clicked() {self.message=match send_independent(a,true){Ok(())=>format!("{}: command sent (physical state unverified)",a.name),Err(e)=>e};}
       if ui.button("Send OFF (manual)").clicked() {self.message=match send_independent(a,false){Ok(())=>format!("{}: OFF command sent (physical state unverified)",a.name),Err(e)=>e};}
      }
      ui.horizontal(|ui|{ui.label("Mode");ui.selectable_value(&mut a.mode,AccessoryMode::OnOff,"ON/OFF");ui.selectable_value(&mut a.mode,AccessoryMode::Pwm,"PWM");ui.checkbox(&mut a.active_high,"Active HIGH (uncheck for LOW)");});
      if a.mode==AccessoryMode::OnOff {
       ui.horizontal(|ui|{ui.label("HIGH command");ui.text_edit_singleline(&mut a.on_command);ui.label("LOW command");ui.text_edit_singleline(&mut a.off_command);});
      } else {
       ui.horizontal(|ui|{ui.label("PWM template");ui.text_edit_singleline(&mut a.pwm_command);ui.label("Duty");ui.add(egui::DragValue::new(&mut a.pwm_value).range(0..=a.pwm_max));ui.label("Max");ui.add(egui::DragValue::new(&mut a.pwm_max).range(1..=65535));});
       ui.small("Template example M7 S{value}; firmware must support PWM. Active LOW inverts duty.");
      }
      ui.horizontal(|ui|{ui.label("Pre-delay ms");ui.add(egui::DragValue::new(&mut a.pre_delay_ms).range(0..=30000));ui.label("Post-delay ms");ui.add(egui::DragValue::new(&mut a.post_delay_ms).range(0..=30000));});
     });
    }
    if let Some(i)=remove {self.accessory_config.outputs.remove(i);}
    ui.collapsing("Phase 34 | Visual block macro editor",|ui|{
      ui.small("Arrange WHEN and DO blocks. Use arrows to reorder actions. Only wired job events execute; input/layer triggers are placeholders. No safety-critical outputs.");
      if ui.button("+ New block program").clicked() && self.macros.rules.len()<256 {
       self.macros.rules.push(MacroRule{name:format!("Program {}",self.macros.rules.len()+1),enabled:false,event:MacroEvent::JobStarted,input_name:String::new(),layer_name:String::new(),actions:Vec::new()});
      }
      let mut remove_macro=None;
      for (i,rule) in self.macros.rules.iter_mut().enumerate(){
       ui.push_id(i,|ui|{
        ui.group(|ui|{
         ui.horizontal(|ui|{ui.checkbox(&mut rule.enabled,"Enabled");ui.label("Program name");ui.text_edit_singleline(&mut rule.name);if ui.small_button("Delete program").clicked(){remove_macro=Some(i);}});
         egui::Frame::group(ui.style()).fill(Color32::from_rgb(40,79,121)).show(ui,|ui|{
          ui.horizontal(|ui|{
           ui.colored_label(Color32::WHITE,"WHEN  ◆");
           egui::ComboBox::from_id_salt("trigger").selected_text(format!("{:?}",rule.event)).show_ui(ui,|ui|{
            for event in [MacroEvent::JobStarted,MacroEvent::JobCompleted,MacroEvent::JobCancelled,MacroEvent::JobFault,MacroEvent::LayerStarted,MacroEvent::InputHigh,MacroEvent::InputLow]{ui.selectable_value(&mut rule.event,event,format!("{:?}",event));}
           });
          });
          if matches!(rule.event,MacroEvent::InputHigh|MacroEvent::InputLow){ui.text_edit_singleline(&mut rule.input_name);ui.colored_label(Color32::YELLOW,"Input monitoring not connected yet");}
          if rule.event==MacroEvent::LayerStarted {ui.text_edit_singleline(&mut rule.layer_name);ui.colored_label(Color32::YELLOW,"Layer event not connected yet");}
         });
         ui.label("↓  DO these actions in order");
         let mut remove_action=None;
         let mut move_action=None;
         let action_count=rule.actions.len();
         for (j,action) in rule.actions.iter_mut().enumerate(){
          ui.push_id(j,|ui|{
           egui::Frame::group(ui.style()).fill(Color32::from_rgb(45,96,77)).show(ui,|ui|{
            ui.horizontal_wrapped(|ui|{
             ui.colored_label(Color32::WHITE,format!("DO {}  ▣",j+1));
             let (mut device,mut kind,mut percent)=match action{MacroAction::SetOn{device}=>(*device,0,100),MacroAction::SetOff{device}=>(*device,1,0),MacroAction::SetPwm{device,percent}=>(*device,2,*percent)};
             egui::ComboBox::from_id_salt("device").selected_text(self.accessory_config.outputs.get(device).map(|a|a.name.as_str()).unwrap_or("Choose device")).show_ui(ui,|ui|{for (index,a) in self.accessory_config.outputs.iter().enumerate(){ui.selectable_value(&mut device,index,&a.name);}});
             egui::ComboBox::from_id_salt("kind").selected_text(["ON","OFF","PWM"][kind]).show_ui(ui,|ui|{for (index,label) in ["ON","OFF","PWM"].iter().enumerate(){ui.selectable_value(&mut kind,index,*label);}});
             if kind==2 {ui.add(egui::Slider::new(&mut percent,0..=100).text("%"));}
             *action=match kind {0=>MacroAction::SetOn{device},1=>MacroAction::SetOff{device},_=>MacroAction::SetPwm{device,percent}};
             if ui.add_enabled(j>0,egui::Button::new("↑")).clicked(){move_action=Some((j,j-1));}
             if ui.add_enabled(j+1<action_count,egui::Button::new("↓")).clicked(){move_action=Some((j,j+1));}
             if ui.small_button("×").clicked(){remove_action=Some(j);}
            });
           });
          });
         }
         if let Some(j)=remove_action {rule.actions.remove(j);} else if let Some((a,b))=move_action {rule.actions.swap(a,b);}
         if ui.button("+ Add DO block").clicked() && rule.actions.len()<32 {rule.actions.push(MacroAction::SetOn{device:0});}
        });
       });
      }
      if let Some(i)=remove_macro{self.macros.rules.remove(i);}
      if let Err(err)=self.macros.validate(&self.accessory_config){ui.colored_label(Color32::YELLOW,err);}
      if ui.button("Save block programs to selected machine").clicked(){if let Some(p)=self.profiles.get_mut(self.active_profile){p["macros"]=json!(self.macros);}self.message=match self.save_profiles(){Ok(())=>"Block macros saved".into(),Err(e)=>e};}
      ui.separator();
      ui.collapsing("Text editor | JSON macro programs", |ui| {
       ui.small("Edit the same macro programs as JSON. Import is transactional: invalid JSON or invalid accessory references leave the block programs unchanged. Text is not automatically overwritten by block edits.");
       ui.horizontal(|ui| {
        if ui.button("Load blocks into text").clicked() {
         match serde_json::to_string_pretty(&self.macros) {
          Ok(value) => {self.macro_text=value;self.macro_text_message="Block programs loaded into text editor".into();}
          Err(err) => self.macro_text_message=format!("Serialization error: {err}"),
         }
        }
        if ui.button("Validate and apply text to blocks").clicked() {
         match serde_json::from_str::<MacroConfig>(&self.macro_text) {
          Ok(candidate) => match candidate.validate(&self.accessory_config) {
           Ok(()) => {self.macros=candidate;self.macro_text_message="Text applied to block programs; save machine profile to persist".into();}
           Err(err) => self.macro_text_message=format!("Validation error: {err}"),
          },
          Err(err) => self.macro_text_message=format!("JSON parse error: {err}"),
         }
        }
       });
       ui.add(egui::TextEdit::multiline(&mut self.macro_text).code_editor().desired_rows(14).desired_width(f32::INFINITY));
       if !self.macro_text_message.is_empty(){ui.label(&self.macro_text_message);}
       ui.small("Example: {\"rules\":[{\"name\":\"Running light\",\"enabled\":true,\"event\":\"JobStarted\",\"input_name\":\"\",\"layer_name\":\"\",\"actions\":[{\"SetOn\":{\"device\":0}}]}]}");
      });
      for entry in self.macro_log.iter().rev().take(5){ui.small(entry);}
    });
    ui.collapsing("Phase 39 | Drag-and-drop routine canvas, event bindings and JSON",|ui|{
      ui.small("Offline routine designer. Conditions, bounded loops, waits, variables and calls. Preview does not actuate hardware. Input values are supplied by the preview context; no live input monitoring.");
      ui.horizontal_wrapped(|ui|{
       if ui.button("+ Template: running light").clicked() && self.routine_db.routines.len()<256 && self.routine_db.bindings.len()<256 {
        let name=format!("Running indicators {}",self.routine_db.routines.len()+1);
        self.routine_db.routines.push(Routine{name:name.clone(),steps:vec![Step::SetOn{device:0},Step::SetPwm{device:1,percent:100}]});
        self.routine_db.bindings.push(EventBinding{event:"JobStarted".into(),routine:name,enabled:true});
       }
       if ui.button("+ Template: job complete").clicked() && self.routine_db.routines.len()<256 && self.routine_db.bindings.len()<256 {
        let name=format!("Completion indicators {}",self.routine_db.routines.len()+1);
        self.routine_db.routines.push(Routine{name:name.clone(),steps:vec![Step::SetOff{device:0},Step::SetOn{device:2},Step::SetPwm{device:1,percent:40}]});
        self.routine_db.bindings.push(EventBinding{event:"JobCompleted".into(),routine:name,enabled:true});
       }
      });
      ui.small("Templates are examples: device indexes must be reviewed. Event bindings are preview-only, not connected to live output.");
      if ui.button("+ Add routine").clicked() && self.routine_db.routines.len()<256 {
        self.routine_db.routines.push(Routine{name:format!("Routine {}",self.routine_db.routines.len()+1),steps:Vec::new()});
      }
      let mut remove=None;
      while self.canvas_editors.len()<self.routine_db.routines.len(){self.canvas_editors.push(Default::default());}
      for (i,r) in self.routine_db.routines.iter_mut().enumerate(){
       ui.push_id(("routine",i),|ui|{ui.collapsing(format!("▣ {}",r.name),|ui|{
        ui.horizontal(|ui|{ui.label("Name");ui.text_edit_singleline(&mut r.name);if ui.button("Delete").clicked(){remove=Some(i);}});
        if let Some(error)=crate::macro_canvas::show(ui, &mut r.steps, &mut self.canvas_drag, &mut self.canvas_editors[i]){self.routine_message=error;}
       });});
      }
      if let Some(i)=remove{self.canvas_editors.remove(i);let name=self.routine_db.routines.remove(i).name;self.routine_db.bindings.retain(|b|b.routine!=name);}
      ui.collapsing("Event → routine bindings (offline)",|ui|{
       if ui.button("+ Bind event").clicked() && self.routine_db.bindings.len()<256 {
        let routine=self.routine_db.routines.first().map(|r|r.name.clone()).unwrap_or_default();
        self.routine_db.bindings.push(EventBinding{event:"JobStarted".into(),routine,enabled:true});
       }
       let mut remove_binding=None;
       for (i,b) in self.routine_db.bindings.iter_mut().enumerate(){
        ui.push_id(("binding",i),|ui|{ui.horizontal(|ui|{
         ui.checkbox(&mut b.enabled,"");
         egui::ComboBox::from_id_salt("event").selected_text(&b.event).show_ui(ui,|ui|{for e in ["JobStarted","JobCompleted","JobCancelled","JobFault","LayerStarted","InputHigh","InputLow"] {ui.selectable_value(&mut b.event,e.into(),e);}});
         ui.label("CALL");ui.text_edit_singleline(&mut b.routine);
         if ui.small_button("Delete").clicked(){remove_binding=Some(i);}
        });});
       }
       if let Some(i)=remove_binding{self.routine_db.bindings.remove(i);}
       if ui.button("Preview JobStarted bindings").clicked(){
        self.routine_message=match self.routine_db.preview_event("JobStarted",Context::default()){Ok(p)=>format!("{} matching routines: {:?}",p.len(),p),Err(e)=>e};
       }
      });
      ui.separator();
      ui.horizontal(|ui|{
       if ui.button("Blocks → JSON").clicked(){match serde_json::to_string_pretty(&self.routine_db){Ok(s)=>{self.routine_text=s;self.routine_message="Loaded routine library into text editor".into();},Err(e)=>self.routine_message=e.to_string()}}
       if ui.button("Validate JSON → Blocks").clicked(){match serde_json::from_str::<RoutineDatabase>(&self.routine_text){Ok(db)=>match db.validate(){Ok(())=>{self.routine_db=db;self.routine_message="Validated and applied".into();},Err(e)=>self.routine_message=e},Err(e)=>self.routine_message=e.to_string()}}
       if ui.button("Save routines to machine").clicked(){if let Some(p)=self.profiles.get_mut(self.active_profile){p["routines"]=json!(self.routine_db);}self.routine_message=match self.save_profiles(){Ok(())=>"Saved".into(),Err(e)=>e};}
      });
      ui.add(egui::TextEdit::multiline(&mut self.routine_text).code_editor().desired_rows(12).desired_width(f32::INFINITY));
      if ui.button("Preview first routine (no hardware commands)").clicked(){
       self.routine_message=if let Some(r)=self.routine_db.routines.first(){match self.routine_db.preview(&r.name,Context::default()){Ok(steps)=>format!("{} planned steps: {:?}",steps.len(),steps.iter().take(12).collect::<Vec<_>>()),Err(e)=>e}}else{"No routines".into()};
      }
      ui.collapsing("Phase 43 | Offline execution trace", |ui| {
       ui.small("Step through a simulated routine timeline. Times represent explicit WAIT blocks only; device latency is not modeled.");
       if let Some(r) = self.routine_db.routines.first() {
        match crate::macro_trace::build_trace(&self.routine_db, &r.name, Context::default()) {
         Ok(trace) => {
          ui.label(format!("{} output actions, {} ms programmed waits", trace.output_operations, trace.total_wait_ms));
          egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
           for entry in trace.entries.iter().take(300) {
            ui.monospace(format!("#{:04}  t={:>7} ms  {}", entry.index + 1, entry.elapsed_ms, entry.description));
           }
           if trace.entries.len() > 300 { ui.small("Showing first 300 operations"); }
          });
         }
         Err(error) => { ui.colored_label(Color32::RED, error); }
        }
       } else { ui.small("Add a routine to see its execution trace."); }
      });
      ui.collapsing("Phase 44 | Offline macro debugger", |ui| {
       ui.small("Simulated inputs and device states only. Breakpoints refer to expanded action indexes (including WAIT). No hardware commands are sent.");
       egui::ComboBox::from_label("Routine to debug").selected_text(if self.debugger.selected_routine.is_empty(){"Choose routine"}else{&self.debugger.selected_routine}).show_ui(ui, |ui| {
        for r in &self.routine_db.routines {ui.selectable_value(&mut self.debugger.selected_routine,r.name.clone(),&r.name);}
       });
       ui.label("Simulated input values (JSON integers, e.g. {\"door_closed\":1})");
       ui.add(egui::TextEdit::multiline(&mut self.debugger.input_json).code_editor().desired_rows(2));
       match self.debugger.plan(&self.routine_db) {
        Ok(plan) => {
         self.debugger.refresh_plan(&plan);
         self.debugger.cursor=self.debugger.cursor.min(plan.len());
         ui.horizontal(|ui| {
          if ui.button("Reset").clicked(){self.debugger.reset();}
          if ui.button("Step").clicked(){self.debugger.next(plan.len());}
          if ui.button("Continue to breakpoint").clicked(){self.debugger.continue_with_conditions(&plan);}
          ui.label(format!("Step {} / {}",self.debugger.cursor,plan.len()));
         });
         ui.collapsing("Conditional breakpoints and watches", |ui| {
          ui.horizontal(|ui| {
           ui.label("Watch device ID (0-255)");
           ui.text_edit_singleline(&mut self.debugger.watch_device);
          });
          ui.horizontal(|ui| {
           ui.label("Break when device ID activated");
           ui.text_edit_singleline(&mut self.debugger.break_device);
           ui.label("PWM at least %");
           ui.add(egui::DragValue::new(&mut self.debugger.break_pwm_at_least).range(0..=100));
          });
          ui.horizontal(|ui| {
           ui.label("Break at elapsed simulated ms (0 disables)");
           ui.add(egui::DragValue::new(&mut self.debugger.break_elapsed_ms).speed(100));
          });
          ui.checkbox(&mut self.debugger.break_on_wait,"Break after WAIT");
          match self.debugger.watched_device(&plan) {
           Ok(Some((id, Some(p)))) => {ui.label(format!("Watch device {id}: {p}% (simulated)"));}
           Ok(Some((id,None))) => {ui.label(format!("Watch device {id}: no command yet"));}
           Ok(None) => {},
           Err(e) => {ui.colored_label(Color32::YELLOW,e);}
          }
         });
         // Clear stale highlights on reset, routine switch, or invalid preview.
         for editor in &mut self.canvas_editors { editor.highlighted_path = None; }
         if let Ok(trace)=self.debugger.context().and_then(|ctx| self.routine_db.traced_preview(&self.debugger.selected_routine,ctx)) {
          ui.horizontal(|ui| {
           ui.label("Break on variable change");
           ui.text_edit_singleline(&mut self.debugger.break_variable_change);
           if ui.button("Continue to change").clicked() {
            if let Some(next)=self.debugger.next_variable_change(&trace) {
             self.debugger.cursor=next;
             self.debugger.paused=true;
            } else { self.debugger.message="No later variable change in offline trace".into(); }
           }
          });
          if !self.debugger.message.is_empty(){ui.small(&self.debugger.message);}
          if let Some(step)=self.debugger.cursor.checked_sub(1).and_then(|i| trace.get(i)) {
           // Translate IF/ELSE and repeat iteration provenance to the same
           // index/branch representation used by the nested canvas editor.
           for (i,r) in self.routine_db.routines.iter().enumerate() {
            let path = if r.name == step.routine {
             crate::macro_source_map::canvas_path(&step.source_path).ok()
              .filter(|path| crate::macro_source_map::valid_canvas_path(&r.steps,path))
            } else {None};
            if let Some(editor)=self.canvas_editors.get_mut(i) {editor.highlighted_path=path;}
           }
          }

          ui.collapsing("Phase 46 | Source blocks, variables and call stack",|ui| {
           ui.horizontal(|ui| {ui.label("Watch variable");ui.text_edit_singleline(&mut self.debugger.watch_variable);});
           if let Some((key,value))=self.debugger.watched_variable(&trace) {
            ui.monospace(format!("{key} = {}",value.map(|v|v.to_string()).unwrap_or_else(||"not assigned yet".into())));
           }
           if let Some(step)=self.debugger.cursor.checked_sub(1).and_then(|i| trace.get(i)) {
            ui.label(format!("Source: {} / {}",step.routine,step.source_path));
            ui.label(format!("Call stack: {}",step.call_stack.join(" → ")));
            ui.collapsing("Variables at this step",|ui|{for (key,value) in &step.variables {ui.monospace(format!("{key} = {value}"));}});
           } else {ui.small("Step once to inspect the source block and variables.");}
           egui::ScrollArea::vertical().max_height(150.0).show(ui,|ui|{
            for (index,step) in trace.iter().enumerate().take(500) {
             if ui.selectable_label(self.debugger.cursor==index+1,format!("{:04} {} / {}",index+1,step.routine,step.source_path)).clicked(){self.debugger.cursor=index+1;}
            }
           });
          });
         }
         let snapshot=self.debugger.snapshot(&plan);
         ui.label(format!("Simulated elapsed: {} ms; actions evaluated: {}",snapshot.elapsed_ms,snapshot.executed));
         ui.collapsing("Simulated output states",|ui|{for (device,value) in &snapshot.devices {ui.monospace(format!("Device {}: {}% (simulated)",device,value.unwrap_or(0)));}});
         egui::ScrollArea::vertical().max_height(200.0).show(ui,|ui| {
          for (i,step) in plan.iter().enumerate().take(500) {
           ui.horizontal(|ui| {
            let mut bp=self.debugger.breakpoints.contains(&i);
            if ui.checkbox(&mut bp,"").changed(){if bp {self.debugger.breakpoints.insert(i);}else{self.debugger.breakpoints.remove(&i);}}
            if ui.selectable_label(i==self.debugger.cursor,format!("{:04} {:?}",i,step)).clicked(){self.debugger.cursor=i;}
           });
          }
          if plan.len()>500{ui.small("Showing first 500 actions; debugger still supports full bounded plan.");}
         });
        }
        Err(e)=>{ui.colored_label(Color32::YELLOW,e);}
       }
      });
      ui.label(&self.routine_message);
      ui.small("Nested IF/ELSE, REPEAT, variables, comparisons and CALL are editable in blocks or JSON. Execution remains offline preview only.");
    });
    if let Err(e)=self.accessory_config.validate(){ui.colored_label(Color32::RED,e);}
    if ui.button("Save accessories to selected machine").clicked(){
     if let Some(p)=self.profiles.get_mut(self.active_profile){p["accessories"]=json!(self.accessory_config);}
     self.message=match self.save_profiles(){Ok(())=>"Accessory profile saved".into(),Err(e)=>e};
    }
    ui.small("These outputs are inserted at job start/end, not switched per layer. They do not guarantee shutdown after alarms, power loss or disconnection.");
   });
   ui.collapsing("Phase 26 | Material and laser calibration tests (offline)",|ui|{
    ui.label("Generate labeled test patterns. Review the G-code in Dry Run before any physical test.");
    let s=&mut self.test_settings;
    egui::ComboBox::from_id_salt("test_kind").selected_text(s.kind.label()).show_ui(ui,|ui|{for kind in TestKind::ALL {ui.selectable_value(&mut s.kind,kind,kind.label());}});
    ui.horizontal(|ui|{ui.label("Rows");ui.add(egui::DragValue::new(&mut s.rows).range(1..=20));ui.label("Columns");ui.add(egui::DragValue::new(&mut s.columns).range(1..=20));});
    ui.horizontal(|ui|{ui.label("Cell mm");ui.add(egui::DragValue::new(&mut s.cell_mm).range(2.0..=100.0));ui.label("Gap mm");ui.add(egui::DragValue::new(&mut s.gap_mm).range(0.0..=50.0));});
    ui.horizontal(|ui|{ui.label("Origin X");ui.add(egui::DragValue::new(&mut s.origin_x));ui.label("Y");ui.add(egui::DragValue::new(&mut s.origin_y));});
    ui.horizontal(|ui|{ui.label("Speed min/max");ui.add(egui::DragValue::new(&mut s.speed_min));ui.add(egui::DragValue::new(&mut s.speed_max));});
    ui.horizontal(|ui|{ui.label("Power min/max %");ui.add(egui::DragValue::new(&mut s.power_min));ui.add(egui::DragValue::new(&mut s.power_max));});
    if matches!(s.kind,TestKind::Interval|TestKind::Grayscale|TestKind::GrayscaleGradient|TestKind::Dither|TestKind::ScanDirection) {ui.horizontal(|ui|{ui.label("Interval min/max mm");ui.add(egui::DragValue::new(&mut s.interval_min));ui.add(egui::DragValue::new(&mut s.interval_max));});}
    if s.kind==TestKind::Passes {ui.horizontal(|ui|{ui.label("Passes min/max");ui.add(egui::DragValue::new(&mut s.passes_min).range(1..=50));ui.add(egui::DragValue::new(&mut s.passes_max).range(1..=50));});}
    if matches!(s.kind,TestKind::GrayscaleGradient|TestKind::Dither) {ui.horizontal(|ui|{ui.label("Gamma");ui.add(egui::DragValue::new(&mut s.gamma).speed(0.05).range(0.1..=5.0));ui.label("Grayscale segments");ui.add(egui::DragValue::new(&mut s.gray_steps).range(2..=256));});ui.small("Gradient: variable PWM within each scanline. Dither: deterministic binary power modulation. Test with material before use.");}
    if ui.button("Generate offline test G-code").clicked(){
     s.bed_width=self.machine_width;s.bed_height=self.machine_height;s.max_s=self.max_s;
     match laser_tests::generate(s).and_then(|code|self.air_config.apply(&code,self.layers[self.selected].air).and_then(|code|self.accessory_config.apply(&code))){Ok(code)=>{self.generated_gcode=code.clone();self.sim_source=code;self.sim_time=0.0;self.sim_playing=false;self.test_status="Generated; inspect in Dry Run. Not sent to controller.".into();},Err(e)=>self.test_status=e}
    }
    ui.label(&self.test_status);
    ui.separator();ui.label("Record measured results for selected machine and material");
    ui.text_edit_multiline(&mut self.test_notes);
    if ui.button("Save calibration result").clicked(){
     let machine=self.profiles.get(self.active_profile).and_then(|p|p["name"].as_str()).unwrap_or("Unknown machine").to_owned();
     let material=self.materials.get(self.active_material).and_then(|p|p["name"].as_str()).unwrap_or("Unknown material").to_owned();
     let result=TestResult{machine,material,settings:self.test_settings.clone(),observations:self.test_notes.clone(),preferred_speed:self.layers[self.selected].speed,preferred_power:self.layers[self.selected].power,preferred_interval:self.layers[self.selected].interval,preferred_passes:self.layers[self.selected].passes};
     self.test_results.push(result);self.test_status=match self.save_profiles(){Ok(())=>"Calibration saved to profiles.json".into(),Err(e)=>format!("Save failed: {e}")};
    }
    for result in self.test_results.iter().rev().take(10){ui.label(format!("{} / {} — {} | {}",result.machine,result.material,result.settings.kind.label(),result.observations));}
   });
   ui.collapsing("Phase 14 | Offline Ruida packet inspector", |ui| {
    ui.colored_label(Color32::YELLOW,"Read-only analysis. This is NOT validated Ruida protocol decoding or active discovery.");
    ui.label("Paste space-separated hexadecimal bytes (from a trusted capture):");
    ui.add(egui::TextEdit::multiline(&mut self.ruida_hex_input).desired_rows(3));
    if ui.button("Inspect offline").clicked() {
     self.ruida_inspection=match ruida_protocol::parse_hex(&self.ruida_hex_input) {
      Ok(bytes)=>{let p=ruida_protocol::inspect(&bytes);format!("Bytes: {} | First: {:?} | First two (BE): {:?} | Sum mod 256: {:02X} | XOR: {:02X} | Last two (BE): {:?}\n{}",p.length,p.first_byte,p.first_two_be,p.byte_sum_mod_256,p.byte_xor,p.trailing_two_be,p.warnings.join("; "))},
      Err(e)=>format!("Invalid input: {e}"),
     };
    }
    ui.monospace(&self.ruida_inspection);
    ui.small("The displayed statistics are descriptive only; they do not establish checksum validity, model identity, or controller state.");
   });
   ui.collapsing("Phase 13 | Ruida passive network inventory (receive only)",|ui|{
    ui.colored_label(Color32::YELLOW,"Experimental passive UDP listener. NOT a Ruida connection or controller driver. No packets are transmitted.");
    ui.horizontal(|ui|{ui.label("Local UDP port");ui.add(egui::DragValue::new(&mut self.ruida_port).range(1024..=65535));});
    ui.horizontal(|ui|{
     if self.ruida_listener.is_none(){if ui.button("Start listening").clicked(){self.ruida_listener=Some(ruida_monitor::Listener::start(self.ruida_port));}}
     else if ui.button("Stop listening").clicked(){self.ruida_listener=None;}
     if ui.button("Clear capture log").clicked(){self.ruida_log.clear();self.ruida_inventory=ruida_monitor::Inventory::default();}
     if ui.button("Copy capture CSV").clicked(){ui.ctx().copy_text(self.ruida_inventory.csv());}
    });
    ui.label(format!("Observed peers: {} | retained packets: {} | discarded oldest: {}",self.ruida_inventory.peers.len(),self.ruida_inventory.captured.len(),self.ruida_inventory.dropped));
    for (ip,peer) in &self.ruida_inventory.peers {ui.monospace(format!("{} | {} packets | {} bytes | {}",ip,peer.packets,peer.bytes,peer.last_fingerprint));}
    ui.small("Observed UDP peers are NOT verified Ruida controllers. No status fields are decoded. The controller may not send unsolicited traffic. A silent capture does not mean a device is absent. Ruida models and protocol versions require validation before active discovery or control.");
    egui::ScrollArea::vertical().max_height(110.0).show(ui,|ui|{for line in self.ruida_log.iter().rev().take(30){ui.monospace(line);}});
   });
   ui.collapsing("Phase 9 | GRBL connection and supervised control",|ui|{
    ui.small("GRBL 1.1 serial devices only. No laser activation, homing, framing or job streaming. Physical E-stop and interlocks remain essential.");
    if ui.button("Refresh serial ports").clicked(){match grbl_monitor::list_ports(){Ok(ports)=>self.detected_ports=ports,Err(e)=>self.message=format!("Port discovery failed: {e}")}}
    egui::ComboBox::from_id_salt("lasercraft_ports").selected_text(if self.connection_port.is_empty(){"Select serial port"}else{&self.connection_port}).show_ui(ui,|ui|{
     for port in &self.detected_ports {ui.selectable_value(&mut self.connection_port,port.clone(),port);}
    });
    ui.horizontal(|ui|{
     if self.monitor.is_none(){if ui.add_enabled(self.controller=="GRBL" && !self.connection_port.is_empty(),egui::Button::new("Connect GRBL")).clicked(){let expect=lasercraft_machine::grbl::Expect{max_s:Some(f64::from(self.max_s)),bed:Some((self.machine_width,self.machine_height))};self.monitor=Some(Monitor::connect(self.connection_port.clone(),self.connection_baud,expect));}}
     else if ui.button("Disconnect").clicked()&& let Some(m)=&self.monitor{m.disconnect();}
    });
    ui.separator();
    ui.label("Phase 9 | GRBL 1.1 supervised control (EXPERIMENTAL)");
    ui.horizontal(|ui|{if ui.button("Identify ($I)").clicked()&& let Some(m)=&self.monitor{m.identify();}
      if ui.button("Read settings ($$)").clicked()&& let Some(m)=&self.monitor{m.settings();}});
    // Unlocking clears an alarm's lock and homing moves the machine: both need motion enabled.
    ui.horizontal(|ui|{let can=self.motion_unlocked && self.monitor.is_some();
      if ui.add_enabled(can,egui::Button::new("Unlock ($X)")).on_hover_text("Clear an alarm lock after checking the machine").clicked()&& let Some(m)=&self.monitor{m.unlock();}
      if ui.add_enabled(can,egui::Button::new("Home ($H)")).on_hover_text("The machine moves to its home switches").clicked()&& let Some(m)=&self.monitor{m.home();}});
    ui.horizontal(|ui|{if ui.button("Feed hold (!)").clicked()&& let Some(m)=&self.monitor{m.hold();}
      if ui.button("Resume (~)").clicked()&& let Some(m)=&self.monitor{m.resume();}});
    ui.checkbox(&mut self.motion_unlocked,"Enable supervised jogging (machine can move)");
    ui.small("Only use with laser physically disabled, enclosure closed, clear work area, and a reachable hardware E-stop. Software controls are not safety interlocks.");
    ui.horizontal(|ui|{ui.label("Distance mm");ui.add(egui::DragValue::new(&mut self.jog_distance).range(0.1..=10.0));ui.label("Feed mm/min");ui.add(egui::DragValue::new(&mut self.jog_feed).range(1.0..=1000.0));});
    let ready=self.motion_unlocked && self.monitor.is_some() && self.monitor_status.state.eq_ignore_ascii_case("Idle") && self.monitor_status.alarm.is_none();
    ui.horizontal_wrapped(|ui|{for (label,axis,sign) in [("X−",'X',-1.0),("X+",'X',1.0),("Y−",'Y',-1.0),("Y+",'Y',1.0),("Z−",'Z',-1.0),("Z+",'Z',1.0)]{
     if ui.add_enabled(ready,egui::Button::new(label)).clicked()&& let Some(m)=&self.monitor&& let Err(e)=m.jog(axis,sign*self.jog_distance,self.jog_feed){self.message=e;}
    }});
    ui.small("Jog is GRBL 1.1 only; position bounds must be enforced by controller soft limits.");
    ui.separator();
    ui.heading("Phase 10 | Supervised GRBL job streaming (EXPERIMENTAL)");
    ui.small("WARNING: Generated G-code may fire the laser. Use only after dry-run validation, with enclosure/interlocks, ventilation, fire supervision, and a physical E-stop. Cancellation issues feed hold, NOT a hardware emergency stop.");
    ui.checkbox(&mut self.stream_armed,"Arm laser job streaming (I have inspected the G-code and machine)");
    let job_active=self.stream_progress.as_ref().is_some_and(|p|p.sent>0&&!p.finished&&p.failed.is_none());
    let can_start=self.stream_armed && !job_active && self.controller=="GRBL" && self.monitor.is_some() && self.monitor_status.state.eq_ignore_ascii_case("Idle") && self.monitor_status.alarm.is_none() && !self.generated_gcode.trim().is_empty();
    ui.horizontal(|ui|{
     if ui.add_enabled(can_start,egui::Button::new("START LASER JOB")).clicked()&& let Some(_m)=&self.monitor{match self.coordinator.start(&self.accessory_config,self.generated_gcode.clone()){Ok(())=>{self.stream_armed=false;self.stream_progress=None;self.message="Starting independent accessories before job".into();},Err(e)=>self.message=e}}
     if ui.add_enabled(job_active,egui::Button::new("Cancel / feed hold")).clicked()&& let Some(m)=&self.monitor{m.cancel_job();self.coordinator.shutdown();}
     if ui.add_enabled(self.monitor.is_some(),egui::Button::new("STOP (hold, then reset)")).on_hover_text("Feed hold, then a soft reset once halted: the laser goes off and the job is dropped. Not an emergency stop.").clicked()&& let Some(m)=&self.monitor{m.stop();self.coordinator.shutdown();}
    });
    ui.label(format!("Independent accessory coordinator: {:?}",self.coordinator.stage));
    if self.coordinator.stage==Stage::Fault && ui.button("Acknowledge accessory fault (verify hardware OFF first)").clicked(){self.coordinator.reset_fault();}
    if matches!(self.coordinator.stage,Stage::Running|Stage::Ready|Stage::Starting) && ui.button("Request accessory shutdown").clicked(){self.coordinator.shutdown();if let Some(m)=&self.monitor{m.cancel_job();}}
    for entry in self.coordinator.log.iter().rev().take(4){ui.small(entry);}
    if let Some(p)=&self.stream_progress{ui.label(format!("GRBL acknowledgements: {} / {} (sent {})",p.acknowledged,p.total,p.sent));ui.add(egui::ProgressBar::new(if p.total==0{0.0}else{p.acknowledged as f32/p.total as f32}).show_percentage());if p.finished{ui.colored_label(Color32::YELLOW,"All lines acknowledged; machine may still be moving. Wait for Idle before approaching.");}
      if let Some(e)=&p.failed{ui.colored_label(Color32::RED,e);}}
    ui.label(format!("State: {}",if self.monitor_status.state.is_empty(){"Unknown"}else{&self.monitor_status.state}));
    if let Some(pos)=self.monitor_status.machine_position {ui.label(format!("Machine X {:.3}  Y {:.3}  Z {:.3} mm",pos[0],pos[1],pos[2]));}
    if let Some(pos)=self.monitor_status.work_position {ui.label(format!("Work X {:.3}  Y {:.3}  Z {:.3} mm",pos[0],pos[1],pos[2]));}
    if let Some(alarm)=&self.monitor_status.alarm{ui.colored_label(Color32::RED,alarm);}
    ui.label(format!("Last status: {}",self.monitor_status.raw));
    egui::ScrollArea::vertical().max_height(90.0).show(ui,|ui|{for line in self.monitor_log.iter().rev().take(15){ui.small(line);}});
    if self.controller=="Ruida"{ui.colored_label(Color32::YELLOW,"Ruida protocol is not implemented.");}
   });
   ui.separator();ui.collapsing("Machine profile",|ui|{
    ui.horizontal(|ui|{ui.label("Bed width (mm)");ui.add(egui::DragValue::new(&mut self.machine_width).range(1.0..=10000.0));});
    ui.horizontal(|ui|{ui.label("Bed height (mm)");ui.add(egui::DragValue::new(&mut self.machine_height).range(1.0..=10000.0));});
    ui.horizontal(|ui|{ui.label("GRBL maximum S");ui.add(egui::DragValue::new(&mut self.max_s).range(1..=100000));});
    ui.horizontal(|ui|{ui.label("Fastest move");ui.add(egui::DragValue::new(&mut self.max_rate).range(60.0..=200_000.0).suffix(" mm/min"));ui.label("Acceleration");ui.add(egui::DragValue::new(&mut self.acceleration).range(10.0..=10_000.0).suffix(" mm/s²"));})
     .response.on_hover_text("For time estimates: GRBL's $110/$111 and $120/$121");
    ui.horizontal(|ui|{
     ui.label("Origin");
     egui::ComboBox::from_id_salt("lasercraft_machine_origin").selected_text(self.machine_origin.label()).show_ui(ui,|ui|{
      for o in Origin::ALL{ui.selectable_value(&mut self.machine_origin,o,o.label());}
     });
    });
    ui.small("The corner the machine homes to and counts from; G-code from the document is placed from that corner of the artboard.");
    if ui.button("Save to selected machine").clicked(){
     if let Some(p)=self.profiles.get_mut(self.active_profile){
      p["width_mm"]=json!(self.machine_width);p["height_mm"]=json!(self.machine_height);p["max_s"]=json!(self.max_s);p["max_rate"]=json!(self.max_rate);p["acceleration"]=json!(self.acceleration);p["origin"]=json!(self.machine_origin.id());
      self.message=match self.save_profiles(){Ok(())=>"Machine profile saved".into(),Err(e)=>format!("Save failed: {e}")};
     } else {self.message="Select a machine first".into();}
    }
   });
   ui.separator();
   ui.collapsing("Generated G-code",|ui|{
    if !self.generated_gcode.is_empty(){
      ui.label(format!("Generated {} bytes of G-code",self.generated_gcode.len()));
      if ui.button("Copy generated G-code").clicked(){ctx.copy_text(self.generated_gcode.clone());}
      egui::ScrollArea::vertical().max_height(100.0).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.generated_gcode).desired_rows(5));});
    } else {ui.small("Generate a job from the document (or a calibration test) to see its G-code here.");}
   });
   ui.separator();
   ui.collapsing("Phase 5 | Fill engraving preview",|ui|{
    ui.small("Offline rectangular scanline test using the selected layer settings. This does not yet fill arbitrary SVG shapes.");
    if ui.button("Generate 20 x 10 mm fill at X10 Y10").clicked(){
     let layer=&self.layers[self.selected];
     match lasercraft_core::advanced_cam::fill_rectangle(10.0,10.0,20.0,10.0,layer.interval,layer.speed,layer.power){
      Ok(job)=>{let stats=lasercraft_core::advanced_cam::statistics(&job);
       match job.to_grbl(Machine{width_mm:self.machine_width,height_mm:self.machine_height,max_s:self.max_s}){
        Ok(code)=>{self.preview=Some(job);self.generated_gcode=match self.air_config.apply(&code,layer.air).and_then(|code|self.accessory_config.apply(&code)){Ok(code)=>code,Err(e)=>{self.message=e;return;}};self.message=format!("Fill: {} segments, {:.1} mm cutting, idealized {:.1} seconds (excludes acceleration and travel)",stats.segments,stats.cut_mm,stats.cut_minutes*60.0);},
        Err(e)=>self.message=format!("Fill validation: {e:?}")}},
      Err(e)=>self.message=e
     }
    }
   });
   ui.separator();
   ui.collapsing("Phase 18 | Dry run, raster exposure and acceleration estimate",|ui|{
    ui.small("OFFLINE simulation only. No commands are sent to the laser. Timeline uses nominal feed time; acceleration estimate is separate and approximate. G2/G3 I/J arcs are tessellated.");
    if ui.button("Load generated G-code").clicked(){self.sim_source=self.generated_gcode.clone();self.sim_step=0;self.sim_time=0.0;self.sim_playing=false;}
    if ui.add(egui::TextEdit::multiline(&mut self.sim_source).desired_rows(5).hint_text("Paste GRBL G-code")).changed(){self.sim_playing=false;self.sim_time=0.0;self.sim_step=0;}
    let sim=laser_sim::simulate(&self.sim_source,self.max_s as f64);
    let n=sim.steps.len();
    if n>0 {
     let duration=sim.duration_seconds;
     // Planned with acceleration and corner speeds (look-ahead), from the machine profile's limits.
     let motion=lasercraft_machine::grbl::Motion{max_rate:[self.max_rate;2],acceleration:[self.acceleration;2],..Default::default()};
     let est=lasercraft_machine::grbl::estimate(&self.sim_source,&motion);
     ui.label(format!("Estimated job time {} ({:.0} mm burning, {:.0} mm travel) | nominal feed time {}",laser_doc::duration(est.seconds),est.burn_mm,est.travel_mm,laser_doc::duration(duration)));
     ui.small(format!("With the machine profile's fastest move ({:.0} mm/min) and acceleration ({:.0} mm/s²); the real time is somewhat longer.",self.max_rate,self.acceleration));
     ui.horizontal(|ui|{ui.label("Exposure resolution");ui.add(egui::Slider::new(&mut self.exposure_resolution,40..=200));ui.checkbox(&mut self.exposure_grayscale,"Grayscale exposure");});
     if self.sim_playing {
      let dt=ctx.input(|i|i.stable_dt).min(0.1) as f64;
      self.sim_time=(self.sim_time+dt*self.sim_rate).min(duration);
      if self.sim_time>=duration {self.sim_playing=false;}
      ctx.request_repaint();
     }
     ui.horizontal(|ui|{
      if ui.button(if self.sim_playing{"Pause"}else{"Play"}).clicked(){if self.sim_time>=duration{self.sim_time=0.0;}self.sim_playing = !self.sim_playing;}
      if ui.button("Stop").clicked(){self.sim_playing=false;self.sim_time=0.0;}
      if ui.button("Previous").clicked(){self.sim_playing=false;self.sim_time=laser_sim::step_start_time(&sim,self.sim_step.saturating_sub(1));}
      if ui.button("Next").clicked(){self.sim_playing=false;self.sim_time=laser_sim::step_end_time(&sim,self.sim_step.min(n-1));}
     });
     ui.horizontal(|ui|{ui.label("Playback");ui.add(egui::Slider::new(&mut self.sim_rate,0.1..=20.0).logarithmic(true).suffix("×"));});
     if ui.add(egui::Slider::new(&mut self.sim_time,0.0..=duration.max(0.001)).text("Time (s)")).changed(){self.sim_playing=false;}
     let state=laser_sim::position_at(&sim,self.sim_time);
     self.sim_step=state.step_index;
     ui.monospace(format!("X {:.3}  Y {:.3}  Z {:.3} mm | {} | {:.1}/{:.1}s",state.x,state.y,state.z,if state.laser_on{"LASER ON"}else{"LASER OFF"},self.sim_time,duration));
     ui.horizontal(|ui|{
      ui.label("Zoom");ui.add(egui::Slider::new(&mut self.sim_zoom,0.5..=12.0).logarithmic(true));
      if ui.button("Fit bed").clicked(){self.sim_zoom=1.0;self.sim_pan=egui::Vec2::ZERO;}
     });
     let (rect,response)=ui.allocate_exact_size(egui::vec2(340.0,240.0),egui::Sense::drag());
     if response.dragged(){self.sim_pan+=response.drag_delta();}
     if response.hovered(){let scroll=ui.input(|i|i.smooth_scroll_delta.y);if scroll!=0.0{self.sim_zoom=(self.sim_zoom*(1.0+scroll*0.001)).clamp(0.5,12.0);}}
     let painter=ui.painter().with_clip_rect(rect);
     painter.rect_filled(rect,0.0,Color32::from_rgb(19,27,35));
     let center=rect.center()+self.sim_pan;
     let sx=rect.width()/self.machine_width.max(1.0) as f32*self.sim_zoom;
     let sy=rect.height()/self.machine_height.max(1.0) as f32*self.sim_zoom;
     // Seen from above with the front edge at the bottom: the machine's 0, 0 sits at its origin corner.
     let (bed_size,origin)=((self.machine_width,self.machine_height),self.machine_origin);
     let project=|x:f64,y:f64|laser_doc::bed_to_screen(center,(sx,sy),bed_size,origin,x,y);
     painter.rect_stroke(laser_doc::bed_rect(center,(sx,sy),bed_size,origin),0.0,egui::Stroke::new(1.0,Color32::GRAY),egui::StrokeKind::Inside);
     let mut previous=(0.0,0.0);
     let mut elapsed=0.0;
     for step in &sim.steps {
      let segment_start=elapsed;
      elapsed+=step.seconds;
      let end_fraction=if step.seconds<=0.0 {if self.sim_time>=elapsed{1.0}else{0.0}}else{((self.sim_time-segment_start)/step.seconds).clamp(0.0,1.0)};
      let from=project(previous.0,previous.1);
      let to=project(step.x,step.y);
      painter.line_segment([from,to],egui::Stroke::new(0.8,Color32::from_gray(85)));
      if end_fraction>0.0 {
       let endpoint=from.lerp(to,end_fraction as f32);
       painter.line_segment([from,endpoint],egui::Stroke::new(2.0,if step.laser_on{Color32::RED}else{Color32::LIGHT_BLUE}));
      }
      previous=(step.x,step.y);
     }
     // Heatmap is a normalized exposure *visualization*, not a burn-depth prediction.
     let cols=self.exposure_resolution;let rows=(cols*7/10).max(1);
     let exposure=laser_sim::exposure_grid(&sim,self.machine_width,self.machine_height,cols,rows,self.sim_time);
     if exposure.peak>0.0 {
      let cell_w=self.machine_width/cols as f64;let cell_h=self.machine_height/rows as f64;
      for row in 0..rows {for col in 0..cols {
       let value=exposure.values[row*cols+col];
       if value>0.0 {
        let alpha=((value/exposure.peak).sqrt()*160.0).clamp(8.0,160.0) as u8;
        let a=project(col as f64*cell_w,row as f64*cell_h);
        let b=project((col+1) as f64*cell_w,(row+1) as f64*cell_h);
        painter.rect_filled(egui::Rect::from_two_pos(a,b),0.0,if self.exposure_grayscale {Color32::from_rgba_unmultiplied(240,240,240,alpha)}else{Color32::from_rgba_unmultiplied(255,160,30,alpha)});
       }
      }}
     }
     let tool=project(state.x,state.y);
     painter.circle_filled(tool,5.0,Color32::YELLOW);
     painter.circle_stroke(tool,8.0,egui::Stroke::new(1.0,Color32::WHITE));
     ui.small("Red: simulated engraving/cutting. Blue: travel. Yellow: tool head. Drag to pan; scroll to zoom.");
     ui.label(format!("G-code: {} moves",n));
     egui::ScrollArea::vertical().max_height(145.0).show(ui,|ui|{
      for (i,step) in sim.steps.iter().enumerate(){
       if ui.selectable_label(i==state.step_index,format!("{:04}  {}",step.line,step.code)).clicked(){self.sim_playing=false;self.sim_time=laser_sim::step_start_time(&sim,i);}
      }
     });
    }else{ui.small("Generate or paste G-code to begin the dry run.");}
    for warning in sim.warnings.iter().take(12){ui.colored_label(Color32::YELLOW,warning);}
    ui.small("G0/G1 and G2/G3 with I/J center offsets. Exposure overlay is relative only; acceleration, firmware behavior, and material response are not modeled.");
   });
   ui.separator();
   ui.collapsing("Phase 6 | Offline toolpath preview",|ui|{
    if let Some(job)=&self.preview {
     let (rect,_) = ui.allocate_exact_size(egui::vec2(320.0,220.0),egui::Sense::hover());
     ui.painter().rect_stroke(rect,0.0,egui::Stroke::new(1.0,Color32::GRAY),egui::StrokeKind::Inside);
     let mut previous=None;
     for op in &job.operations {
      let (point,cut)=match op {Operation::Travel(p)=>(*p,false),Operation::Cut{to,..}=>(*to,true),Operation::LaserOff=>{previous=None;continue}};
      let position=egui::pos2(rect.left()+point.x_mm as f32/self.machine_width as f32*rect.width(),rect.top()+point.y_mm as f32/self.machine_height as f32*rect.height());
      if let Some(start)=previous {ui.painter().line_segment([start,position],egui::Stroke::new(if cut{1.5}else{0.6},if cut{Color32::RED}else{Color32::LIGHT_BLUE}));}
      previous=Some(position);
     }
     ui.small("Red: laser-on moves. Blue: travel. Preview is not a machine simulation.");
    } else {ui.small("Generate a job to see its offline toolpaths.");}
   });
   ui.separator();
   ui.label("Offline G-code diagnostic: 10 × 10 mm square at X10 Y10. This is NOT document artwork.");
   if ui.button("Copy diagnostic GRBL G-code").clicked(){
    let machine=Machine{width_mm:self.machine_width,height_mm:self.machine_height,max_s:self.max_s};
    let layer=&self.layers[self.selected];
    let mut ops=vec![Operation::Travel(Point{x_mm:10.0,y_mm:10.0})];
    for _ in 0..layer.passes {for (x,y) in [(20.0,10.0),(20.0,20.0),(10.0,20.0),(10.0,10.0)] {ops.push(Operation::Cut{to:Point{x_mm:x,y_mm:y},speed_mm_min:layer.speed,power_percent:layer.power});}ops.push(Operation::LaserOff);}
    match (Job{operations:ops}).to_grbl(machine){Ok(code)=>{ctx.copy_text(code);self.message="Diagnostic G-code copied. Inspect before use; laser power commands are present.".into();},Err(e)=>self.message=format!("Cannot export: {e:?}")}
   }
   ui.small(&self.message);
   ui.colored_label(Color32::YELLOW,"No motion or laser firing commands. GRBL monitoring only; Ruida and raster are not implemented.");
  });
        self.open = open;
    }
}

impl LaserPanel {
    pub fn camera_gcode(&self) -> &str {
        if self.sim_source.is_empty() { &self.generated_gcode } else { &self.sim_source }
    }
    pub fn camera_bed_width(&self) -> f64 {
        self.machine_width
    }
    pub fn camera_bed_height(&self) -> f64 {
        self.machine_height
    }
    pub fn camera_max_s(&self) -> f64 {
        self.max_s as f64
    }
}

// Phase 37: recursive, bounded block editor. This modifies the same AST as the JSON editor.
pub(crate) fn edit_condition(ui: &mut egui::Ui, expr: &mut crate::macro_routines::Expr, depth: usize) {
    use crate::macro_routines::Expr;
    if depth > 10 {
        ui.label("Expression nesting limit");
        return;
    }
    let mut kind = match expr {
        Expr::True => 0,
        Expr::False => 1,
        Expr::InputEquals { .. } => 2,
        Expr::VariableEquals { .. } => 3,
        Expr::And(_) => 4,
        Expr::Or(_) => 5,
        Expr::Not(_) => 6,
    };
    egui::ComboBox::from_id_salt(("condition_kind", depth, expr as *const _ as usize))
        .selected_text(["TRUE", "FALSE", "INPUT =", "VARIABLE =", "AND", "OR", "NOT"][kind])
        .show_ui(ui, |ui| {
            for (i, label) in ["TRUE", "FALSE", "INPUT =", "VARIABLE =", "AND", "OR", "NOT"].iter().enumerate() {
                ui.selectable_value(&mut kind, i, *label);
            }
        });
    let old_kind = match expr {
        Expr::True => 0,
        Expr::False => 1,
        Expr::InputEquals { .. } => 2,
        Expr::VariableEquals { .. } => 3,
        Expr::And(_) => 4,
        Expr::Or(_) => 5,
        Expr::Not(_) => 6,
    };
    if kind != old_kind {
        *expr = match kind {
            0 => Expr::True,
            1 => Expr::False,
            2 => Expr::InputEquals { key: "sensor".into(), value: 1 },
            3 => Expr::VariableEquals { key: "counter".into(), value: 1 },
            4 => Expr::And(vec![Expr::True, Expr::True]),
            5 => Expr::Or(vec![Expr::True, Expr::False]),
            _ => Expr::Not(Box::new(Expr::True)),
        };
    }
    match expr {
        Expr::InputEquals { key, value } | Expr::VariableEquals { key, value } => {
            ui.text_edit_singleline(key);
            ui.add(egui::DragValue::new(value));
        }
        Expr::And(xs) | Expr::Or(xs) => {
            let mut remove = None;
            for (i, x) in xs.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}", i + 1));
                    edit_condition(ui, x, depth + 1);
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                xs.remove(i);
            }
            if xs.len() < 16 && ui.small_button("+ condition").clicked() {
                xs.push(Expr::True);
            }
        }
        Expr::Not(x) => edit_condition(ui, x, depth + 1),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jobs from the document are made for a loaded machine's bed only: none is guessed before.
    #[test]
    fn no_bed_until_a_machine_profile_is_loaded() {
        let mut p = LaserPanel { profiles: vec![], loaded_profile: None, ..LaserPanel::default() };
        assert_eq!(p.bed(), Ok(None));
        p.profiles = vec![json!({"name": "Shop CO2", "controller": "Ruida", "width_mm": 600.0, "height_mm": 400.0, "max_s": 1000})];
        p.active_profile = 0;
        p.apply_machine();
        assert_eq!(p.loaded_profile.as_deref(), Some("Shop CO2"));
        let bed = Bed {
            width_mm: 600.0,
            height_mm: 400.0,
            max_s: 1000,
            origin: Origin::TopRight,
            prologue: vec![],
            epilogue: vec![],
            max_rate: 6000.0,
            acceleration: 500.0,
        };
        assert_eq!(p.bed(), Ok(Some(bed.clone())));
        // With air assist on, the bed carries its G-code for the command.
        p.air_config = AirConfig { enabled: true, default_on: true, ..AirConfig::default() };
        let with_air = p.bed().unwrap().unwrap();
        assert!(with_air.prologue.iter().any(|l| l == "M8") && with_air.epilogue.iter().any(|l| l == "M9"));
        // Refused settings are an error, not a job without them.
        p.air_config.on_command = "G1 X5; M8".into();
        assert!(p.bed().is_err());
    }

    /// Asks `method` until `done` holds for its result (the monitor's thread answers in its own
    /// time), for up to 5 s.
    fn until(p: &mut LaserPanel, method: &str, done: impl Fn(&Value) -> bool) -> Value {
        let start = std::time::Instant::now();
        loop {
            let v = p.automation(method, &json!({})).unwrap();
            if done(&v) || start.elapsed() > std::time::Duration::from_secs(5) {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// On the simulator, automation drives the whole lifecycle: connect, status, jog, home,
    /// unlock, start a job and see it finish, stop, disconnect.
    #[test]
    fn automation_drives_the_simulator() {
        let mut p = LaserPanel { monitor: None, ..LaserPanel::default() };
        assert!(p.automation("laser.machine.jog", &json!({"axis": "x", "distance": 1})).unwrap_err().contains("no machine"));
        let r = p.automation("laser.machine.connect", &json!({"simulator": true})).unwrap();
        assert_eq!(r["link"], "simulator");
        assert!(p.automation("laser.machine.connect", &json!({"simulator": true})).is_err(), "already connected");
        let s = until(&mut p, "laser.machine.status", |v| v["state"] == "Idle");
        assert_eq!(s["state"], "Idle");
        for (method, params) in [
            ("laser.machine.jog", json!({"axis": "x", "distance": 2, "feed": 300})),
            ("laser.machine.home", json!({})),
            ("laser.machine.unlock", json!({})),
            ("laser.machine.readSettings", json!({})),
        ] {
            assert_eq!(p.automation(method, &params).unwrap()["link"], "simulator", "{method}");
        }
        until(&mut p, "laser.machine.status", |v| v["state"] == "Idle");
        p.automation("laser.machine.start", &json!({"gcode": "G21\nG90\nG0 X5 Y5\nM4 S100\nG1 X10 F600\nM5"})).unwrap();
        let s = until(&mut p, "laser.machine.status", |v| v["job"]["finished"] == true);
        assert_eq!(s["job"]["acknowledged"], 6, "{s}");
        p.automation("laser.machine.hold", &json!({})).unwrap();
        p.automation("laser.machine.stop", &json!({})).unwrap();
        p.automation("laser.machine.disconnect", &json!({})).unwrap();
        until(&mut p, "laser.machine.status", |v| v["connected"] == false);
        assert!(p.automation("laser.other", &json!({})).is_err());
        assert!(p.automation("laser.machine.fly", &json!({})).is_err());
    }

    /// On a real machine (a serial link; here the simulator stands behind a monitor opened as
    /// serial), automation may only read, hold and stop: every motion request is refused, and no
    /// param changes that.
    #[test]
    fn automation_cannot_move_a_real_machine() {
        let expect = lasercraft_machine::grbl::Expect::default();
        let real = Monitor::spawn(Link::Serial, expect, || {
            Ok(Box::new(lasercraft_machine::sim::SimulatedGrbl::default()) as Box<dyn lasercraft_machine::Transport + Send>)
        });
        let mut p = LaserPanel { monitor: Some(real), ..LaserPanel::default() };
        until(&mut p, "laser.machine.status", |v| v["state"] == "Idle");
        assert_eq!(p.automation("laser.machine.status", &json!({})).unwrap()["link"], "serial");
        for (method, params) in [
            ("laser.machine.jog", json!({"axis": "x", "distance": 1})),
            ("laser.machine.home", json!({})),
            ("laser.machine.unlock", json!({})),
            ("laser.machine.resume", json!({})),
            ("laser.machine.start", json!({"gcode": "G21\nG0 X1"})),
            // Asking for the simulator, or claiming to be the panel, changes nothing.
            ("laser.machine.start", json!({"gcode": "G21\nG0 X1", "simulator": true, "link": "simulator", "panel": true})),
            ("laser.machine.jog", json!({"axis": "x", "distance": 1, "allowMotion": true})),
        ] {
            let e = p.automation(method, &params).unwrap_err();
            assert!(e.contains("panel-only"), "{method}: {e}");
        }
        for method in ["laser.machine.readSettings", "laser.machine.hold", "laser.machine.stop"] {
            assert_eq!(p.automation(method, &json!({})).unwrap()["link"], "serial", "{method}");
        }
        // Nothing reached the controller: no job ever started.
        assert!(p.automation("laser.machine.status", &json!({})).unwrap()["job"].is_null());
        p.automation("laser.machine.disconnect", &json!({})).unwrap();
    }

    fn unloaded() -> LaserPanel {
        LaserPanel { profiles: vec![], materials: vec![], loaded_profile: None, active_profile: 0, ..LaserPanel::default() }
    }

    /// The machine loaded last is loaded again at start; a missing or broken one leaves none
    /// loaded (the document section asks for one) without failing.
    #[test]
    fn the_last_machine_is_loaded_at_start() {
        let machines = json!([
            {"name": "GRBL 300 x 200", "controller": "GRBL", "width_mm": 300.0, "height_mm": 200.0, "max_s": 1000},
            {"name": "Shop CO2", "controller": "Ruida", "width_mm": 600.0, "height_mm": 400.0, "origin": "bottomRight"},
            {"name": "Broken", "width_mm": "wide", "height_mm": 400.0}
        ]);
        let p = unloaded().load_profiles_from(&json!({"machines": machines, "active_machine": "Shop CO2"}));
        assert_eq!(p.loaded_profile.as_deref(), Some("Shop CO2"));
        assert_eq!(p.active_profile, 1);
        let bed = p.bed().unwrap().unwrap();
        assert_eq!((bed.width_mm, bed.height_mm, bed.origin), (600.0, 400.0, Origin::BottomRight));
        for active in [json!("Gone"), json!("Broken"), json!(42), Value::Null] {
            let p = unloaded().load_profiles_from(&json!({"machines": machines, "active_machine": active}));
            assert_eq!(p.loaded_profile, None, "{active}");
            assert_eq!(p.bed(), Ok(None));
        }
        let p = unloaded().load_profiles_from(&json!({"machines": machines, "active_machine": "Gone"}));
        assert!(p.message.contains("Gone"), "{}", p.message);
        // No file: the example machine, none loaded.
        let p = unloaded().load_profiles_from(&Value::Null);
        assert_eq!((p.profiles.len(), p.loaded_profile.clone()), (1, None));
    }

    /// A job from the document goes to the dry run, with the machine's (default, empty) hooks.
    #[test]
    fn a_document_job_goes_to_the_dry_run() {
        let mut p = LaserPanel::default();
        p.take_document_job("G21\nG90\nM5\nG0 X1 Y1\nM5\n".into());
        assert!(p.generated_gcode.contains("G0 X1 Y1"));
        assert_eq!(p.sim_source, p.generated_gcode);
        assert!(p.preview.is_none());
    }
}
