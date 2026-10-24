//! LaserCraft Phase 19: offline USB-camera snapshot alignment and G-code placement preview.
//! Calibration is visual-only; it cannot certify real-world alignment or machine safety.
use crate::camera_calibration::Homography;
use crate::camera_lens::Lens;
use crate::camera_validation;
use crate::laser_sim;
use egui::{self, Color32, Pos2, Rect, Sense, Stroke, TextureHandle};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub struct CameraOverlay {
    pub open: bool,
    source: String,
    device: String,
    image_path: String,
    texture: Option<TextureHandle>,
    image_size: [usize; 2],
    corners: Vec<[f32; 2]>, // normalized image-space: bed BL, BR, TR, TL
    calibrating: bool,
    opacity: f32,
    error: String,
    show_travel: bool,
    offset_x: f64,
    offset_y: f64,
    live: bool,
    frame_receiver: Option<Receiver<Result<Vec<u8>, String>>>,
    last_capture: Option<Instant>,
    capture_interval_ms: u64,
    drag_origin: Option<(f64, f64)>,
    lens: Lens,
    lens_enabled: bool,
    validation_csv: String,
    validation_report: String,
}
impl Default for CameraOverlay {
    fn default() -> Self {
        Self {
            open: false,
            source: "File / snapshot".into(),
            device: "/dev/video0".into(),
            image_path: String::new(),
            texture: None,
            image_size: [0, 0],
            corners: Vec::new(),
            calibrating: false,
            opacity: 0.7,
            error: String::new(),
            show_travel: true,
            offset_x: 0.0,
            offset_y: 0.0,
            live: false,
            frame_receiver: None,
            last_capture: None,
            capture_interval_ms: 750,
            drag_origin: None,
            lens: Lens::default(),
            lens_enabled: false,
            validation_csv: String::new(),
            validation_report: String::new(),
        }
    }
}
impl CameraOverlay {
    fn load(&mut self, ctx: &egui::Context, path: PathBuf) {
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.texture = Some(ctx.load_texture("lasercraft_camera_snapshot", ci, egui::TextureOptions::LINEAR));
                self.image_size = size;
                self.image_path = path.to_string_lossy().into_owned();
                self.corners.clear();
                self.calibrating = false;
                self.error.clear();
            }
            Err(e) => self.error = format!("Cannot decode camera image: {e}"),
        }
    }
    fn capture(&mut self, ctx: &egui::Context) {
        // ffmpeg must be installed. Capture is a single still, not a live feed.
        let out = std::env::temp_dir().join("lasercraft-camera-snapshot.png");
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-hide_banner").arg("-loglevel").arg("error").arg("-y");
        if cfg!(target_os = "linux") {
            cmd.args(["-f", "v4l2", "-i", &self.device]);
        } else if cfg!(target_os = "windows") {
            cmd.args(["-f", "dshow", "-i", &format!("video={}", self.device)]);
        } else {
            cmd.args(["-f", "avfoundation", "-i", &self.device]);
        }
        cmd.args(["-frames:v", "1"]);
        cmd.arg(&out);
        match cmd.output() {
            Ok(o) if o.status.success() => self.load(ctx, out),
            Ok(o) => self.error = format!("Camera capture failed: {}", String::from_utf8_lossy(&o.stderr)),
            Err(e) => self.error = format!("Cannot start ffmpeg: {e}"),
        }
    }
    fn start_async_capture(&mut self) {
        if self.frame_receiver.is_some() {
            return;
        }
        let device = self.device.clone();
        let (tx, rx) = mpsc::channel();
        self.frame_receiver = Some(rx);
        self.last_capture = Some(Instant::now());
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("ffmpeg");
            cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
            if cfg!(target_os = "linux") {
                cmd.args(["-f", "v4l2", "-i", &device]);
            } else if cfg!(target_os = "windows") {
                cmd.args(["-f", "dshow", "-i", &format!("video={device}")]);
            } else {
                cmd.args(["-f", "avfoundation", "-i", &device]);
            }
            cmd.args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "mjpeg", "pipe:1"]);
            let result = cmd
                .output()
                .map_err(|e| e.to_string())
                .and_then(|out| if out.status.success() { Ok(out.stdout) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) });
            let _ = tx.send(result);
        });
    }
    fn poll_capture(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.frame_receiver {
            match rx.try_recv() {
                Ok(result) => {
                    self.frame_receiver = None;
                    match result {
                        Ok(bytes) => match image::load_from_memory(&bytes) {
                            Ok(img) => {
                                let rgba = img.to_rgba8();
                                let size = [rgba.width() as usize, rgba.height() as usize];
                                let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                self.texture = Some(ctx.load_texture("lasercraft_live_camera", ci, egui::TextureOptions::LINEAR));
                                if self.image_size != size {
                                    self.corners.clear();
                                }
                                self.image_size = size;
                                self.error.clear();
                            }
                            Err(e) => self.error = format!("Cannot decode webcam frame: {e}"),
                        },
                        Err(e) => {
                            self.error = format!("Webcam capture failed: {e}");
                            self.live = false;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.frame_receiver = None;
                    self.live = false;
                    self.error = "Camera capture worker stopped".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.live {
            let due = self.last_capture.map_or(true, |t| t.elapsed() >= Duration::from_millis(self.capture_interval_ms));
            if due && self.frame_receiver.is_none() {
                self.start_async_capture();
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, gcode: &str, bed_w: f64, bed_h: f64, max_s: f64) {
        if !self.open {
            self.live = false;
            return;
        }
        self.poll_capture(ctx);
        let mut open = self.open;
        egui::Window::new("LaserCraft | Camera Placement").open(&mut open).default_width(690.0).resizable(true).show(ctx,|ui|{
            ui.label("USB webcam snapshot / workpiece placement. OFFLINE preview only; never moves or fires the laser.");
            ui.horizontal(|ui|{
                if ui.button("Open image…").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Images", &["png","jpg","jpeg","webp"]).pick_file(){self.load(ctx,path);}}
                if ui.button("Capture USB webcam still").clicked(){self.capture(ctx);}
            });
            ui.horizontal(|ui|{ui.label("Camera device");ui.text_edit_singleline(&mut self.device);});
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.live,"Live USB preview (repeated snapshots)");
                ui.add(egui::Slider::new(&mut self.capture_interval_ms,250..=3000).text("Refresh ms"));
                if self.frame_receiver.is_some() {ui.spinner();}
            });
            ui.collapsing("Lens correction (experimental / manually calibrated)", |ui| {
                ui.checkbox(&mut self.lens_enabled,"Enable Brown-Conrady correction");
                ui.horizontal(|ui| {ui.label("Radial k1/k2");ui.add(egui::DragValue::new(&mut self.lens.k1).speed(0.001));ui.add(egui::DragValue::new(&mut self.lens.k2).speed(0.001));});
                ui.horizontal(|ui| {ui.label("Tangential p1/p2");ui.add(egui::DragValue::new(&mut self.lens.p1).speed(0.0001));ui.add(egui::DragValue::new(&mut self.lens.p2).speed(0.0001));});
                ui.horizontal(|ui| {ui.label("Principal cx/cy");ui.add(egui::DragValue::new(&mut self.lens.cx).speed(0.001));ui.add(egui::DragValue::new(&mut self.lens.cy).speed(0.001));});
                ui.horizontal(|ui| {ui.label("Focal fx/fy");ui.add(egui::DragValue::new(&mut self.lens.fx).speed(0.001));ui.add(egui::DragValue::new(&mut self.lens.fy).speed(0.001));});
                if ui.button("Import automatic checkerboard calibration…").clicked() {
                    if let Some(path)=rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                        let result=(|| -> Result<(Lens,f64),String> {
                            let raw=std::fs::read_to_string(path).map_err(|e|e.to_string())?;
                            let v:serde_json::Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
                            if v["format"].as_str()!=Some("lasercraft-intrinsics-v1") || v["accepted"].as_bool()!=Some(true) {return Err("Calibration not accepted".into());}
                            let dims=v["image_size"].as_array().ok_or("Missing image dimensions")?;
                            if dims.len()!=2 || dims[0].as_u64()!=Some(self.image_size[0] as u64) || dims[1].as_u64()!=Some(self.image_size[1] as u64) {return Err("Calibration image resolution differs from current camera".into());}
                            let rms=v["rms_px"].as_f64().ok_or("Missing RMS error")?;
                            if !rms.is_finite() || rms>1.0 {return Err("RMS exceeds 1 pixel safety threshold".into());}
                            let values=v["lens"].as_array().ok_or("Missing lens coefficients")?;
                            if values.len()!=8 {return Err("Expected 8 lens parameters".into());}
                            let mut a=[0.0;8];for (i,x) in values.iter().enumerate(){a[i]=x.as_f64().ok_or("Invalid lens coefficient")?;}
                            let lens=Lens{k1:a[0],k2:a[1],p1:a[2],p2:a[3],cx:a[4],cy:a[5],fx:a[6],fy:a[7]};
                            if !lens.valid() {return Err("Invalid lens model".into());}
                            if v["k3"].as_f64().unwrap_or(0.0).abs()>0.01 {return Err("k3 distortion is significant but unsupported; use a narrower lens".into());}
                            Ok((lens,rms))
                        })();
                        match result {Ok((lens,rms))=>{self.lens=lens;self.lens_enabled=true;self.corners.clear();self.error=format!("Imported lens calibration (RMS {rms:.3}px). Recalibrate bed corners and validate independent points.");},Err(e)=>self.error=format!("Calibration import rejected: {e}")}
                    }
                }
                if ui.button("Reset lens coefficients").clicked(){self.lens=Lens::default();self.lens_enabled=false;}
                ui.small("Coefficients must be measured from a calibration target. This is not automatic checkerboard detection.");
            });
            ui.small("Live mode captures independent frames via FFmpeg; this is NOT a continuous video stream. Keep the camera fixed after calibration.");
            ui.small("Linux: /dev/video0 · Windows: camera name (DirectShow) · macOS: device index (AVFoundation). Requires ffmpeg on PATH.");
            if !self.error.is_empty(){ui.colored_label(Color32::LIGHT_RED,&self.error);}
            ui.horizontal(|ui|{
                if ui.button("Calibrate four bed corners").clicked(){self.corners.clear();self.calibrating=true;}
                if ui.button("Reset calibration").clicked(){self.corners.clear();self.calibrating=false;}
                ui.add(egui::Slider::new(&mut self.opacity,0.05..=1.0).text("Image opacity"));
                ui.checkbox(&mut self.show_travel,"Travel paths");
            });
            const LABELS:[&str;4]=["bottom-left (X0 Y0)","bottom-right (Xmax Y0)","top-right (Xmax Ymax)","top-left (X0 Ymax)"];
            if self.calibrating {ui.strong(format!("Click {} on the camera image",LABELS[self.corners.len().min(3)]));}
            if self.corners.len()==4 {ui.colored_label(Color32::LIGHT_GREEN,"Projective bed mapping active (lens distortion NOT corrected). Verify against measured reference points.");}
            ui.horizontal(|ui| {ui.label("Artwork placement offset (mm)");ui.add(egui::DragValue::new(&mut self.offset_x).speed(0.1).prefix("X "));ui.add(egui::DragValue::new(&mut self.offset_y).speed(0.1).prefix("Y "));});
            ui.horizontal(|ui| {
                if ui.button("Save calibration…").clicked() && self.corners.len()==4 {
                    if let Some(path)=rfd::FileDialog::new().add_filter("JSON", &["json"]).save_file(){
                        let data=serde_json::json!({"version":1,"image_size":self.image_size,"bed_width_mm":bed_w,"bed_height_mm":bed_h,"corners":self.corners,"lens_enabled":self.lens_enabled,"lens":[self.lens.k1,self.lens.k2,self.lens.p1,self.lens.p2,self.lens.cx,self.lens.cy,self.lens.fx,self.lens.fy]});
                        if let Err(e)=std::fs::write(path,data.to_string()){self.error=e.to_string();}
                    }
                }
                if ui.button("Load calibration…").clicked(){
                    if let Some(path)=rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file(){
                        match std::fs::read_to_string(path).ok().and_then(|s|serde_json::from_str::<serde_json::Value>(&s).ok()) {
                            Some(v) => {
                                let parsed=(|| -> Option<Vec<[f32;2]>> {
                                    if v["version"].as_u64()? != 1 || (v["bed_width_mm"].as_f64()? - bed_w).abs()>0.01 || (v["bed_height_mm"].as_f64()? - bed_h).abs()>0.01 {return None;}
                                    let sz=v["image_size"].as_array()?;
                                    if sz.len()!=2 || sz[0].as_u64()? as usize != self.image_size[0] || sz[1].as_u64()? as usize != self.image_size[1] {return None;}
                                    let mut pts=Vec::new();for p in v["corners"].as_array()? {pts.push([p[0].as_f64()? as f32,p[1].as_f64()? as f32]);}
                                    if pts.len()!=4 {return None;}Some(pts)
                                })();
                                if let Some(c)=parsed {
                                    self.corners=c;self.calibrating=false;
                                    if let Some(a)=v["lens"].as_array() {if a.len()==8 {
                                        let vals:Vec<f64>=a.iter().filter_map(|x|x.as_f64()).collect();
                                        if vals.len()==8 {let lens=Lens{k1:vals[0],k2:vals[1],p1:vals[2],p2:vals[3],cx:vals[4],cy:vals[5],fx:vals[6],fy:vals[7]};if lens.valid(){self.lens=lens;self.lens_enabled=v["lens_enabled"].as_bool().unwrap_or(false);}}
                                    }}
                                } else {self.error="Calibration does not match current image size or bed dimensions".into();}
                            },None=>self.error="Cannot read calibration JSON".into()
                        }
                    }
                }
            });
            let Some(tex)=&self.texture else {ui.label("Open an image or capture a webcam still to begin.");return;};
            let size=tex.size_vec2();let width=ui.available_width().min(760.0).max(250.0);
            let desired=egui::vec2(width,width*size.y/size.x);
            let (rect,response)=ui.allocate_exact_size(desired,Sense::click_and_drag());
            ui.painter().image(tex.id(),rect,Rect::from_min_max(Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE.gamma_multiply(self.opacity));
            if self.calibrating && response.clicked(){
                if let Some(pos)=response.interact_pointer_pos(){
                    self.corners.push([((pos.x-rect.min.x)/rect.width()).clamp(0.0,1.0),((pos.y-rect.min.y)/rect.height()).clamp(0.0,1.0)]);
                    if self.corners.len()==4 {self.calibrating=false;}
                }
            }
            for (i,p) in self.corners.iter().enumerate(){
                let pos=egui::pos2(rect.left()+p[0]*rect.width(),rect.top()+p[1]*rect.height());
                ui.painter().circle_filled(pos,5.0,Color32::YELLOW);
                ui.painter().text(pos+egui::vec2(7.0,-8.0),egui::Align2::LEFT_TOP,format!("{}",i+1),egui::FontId::proportional(13.0),Color32::WHITE);
            }
            if self.corners.len()!=4{return;}
            let corners=[self.corners[0],self.corners[1],self.corners[2],self.corners[3]];
            let corrected_corners = if self.lens_enabled {
                let mut c=corners;
                for p in &mut c {if let Some((u,v))=self.lens.undistort(p[0] as f64,p[1] as f64){*p=[u as f32,v as f32];}else{ui.colored_label(Color32::LIGHT_RED,"Lens correction failed");return;}}
                c
            } else {corners};
            let calibration=match Homography::from_bed_corners(&corrected_corners,bed_w,bed_h){Ok(h)=>h,Err(e)=>{ui.colored_label(Color32::LIGHT_RED,e);return;}};
            ui.collapsing("Calibration accuracy validation (independent points)", |ui| {
                ui.small("Enter measured reference points: bed_x_mm,bed_y_mm,image_u,image_v. Image U/V must be normalized to 0–1. Do not reuse the four calibration corners.");
                ui.add(egui::TextEdit::multiline(&mut self.validation_csv).desired_rows(4).hint_text("bed_x_mm,bed_y_mm,image_u,image_v\n50,50,0.15,0.75\n150,50,0.45,0.75\n100,100,0.30,0.50"));
                if ui.button("Evaluate calibration error").clicked() {
                    self.validation_report = match camera_validation::parse_points(&self.validation_csv).and_then(|pts| camera_validation::evaluate(&calibration, if self.lens_enabled {Some(&self.lens)} else {None}, &pts,self.image_size[0],self.image_size[1])) {
                        Ok(q) => format!("{} independent points | RMS {:.2} px / {:.3} mm | Max {:.2} px / {:.3} mm. Compare against your required tolerance; these measurements do not certify machine accuracy.",q.count,q.rms_px,q.rms_mm,q.max_px,q.max_mm),
                        Err(e) => format!("Validation error: {e}"),
                    };
                }
                ui.label(&self.validation_report);
            });
            let map=|x:f64,y:f64| -> Pos2 {
                let (mut u,mut v)=calibration.project(x,y).unwrap_or((-10.0,-10.0));
                if self.lens_enabled {(u,v)=self.lens.distort(u,v).unwrap_or((-10.0,-10.0));}
                egui::pos2(rect.left()+u as f32*rect.width(),rect.top()+v as f32*rect.height())
            };
            if !self.calibrating {
                if response.drag_started() {self.drag_origin=Some((self.offset_x,self.offset_y));}
                if response.dragged() {
                    if let Some((ox,oy))=self.drag_origin {
                        let d=response.drag_delta();
                        let du=d.x as f64/rect.width() as f64;
                        let dv=d.y as f64/rect.height() as f64;
                        let lens_inverse=|u:f64,v:f64| -> Option<(f64,f64)> {
                            let (u,v)=if self.lens_enabled {self.lens.undistort(u,v)?}else{(u,v)};
                            calibration.unproject(u,v)
                        };
                        if let (Some(a),Some(b))=(lens_inverse(0.5,0.5),lens_inverse(0.5+du,0.5+dv)) {
                            self.offset_x=ox+b.0-a.0;
                            self.offset_y=oy+b.1-a.1;
                        }
                    }
                }
                if response.drag_stopped() {self.drag_origin=None;}
            }
            let painter=ui.painter().with_clip_rect(rect);
            for i in 0..4 {painter.line_segment([map([0.0,bed_w,bed_w,0.0][i],[0.0,0.0,bed_h,bed_h][i]),map([0.0,bed_w,bed_w,0.0][(i+1)%4],[0.0,0.0,bed_h,bed_h][(i+1)%4])],Stroke::new(1.5,Color32::YELLOW));}
            let sim=laser_sim::simulate(gcode,max_s);
            let mut prev=(0.0,0.0);
            for step in &sim.steps {
                if step.laser_on || self.show_travel {painter.line_segment([map(prev.0+self.offset_x,prev.1+self.offset_y),map(step.x+self.offset_x,step.y+self.offset_y)],Stroke::new(if step.laser_on{2.0}else{0.8},if step.laser_on{Color32::RED}else{Color32::LIGHT_BLUE}));}
                prev=(step.x,step.y);
            }
            ui.label(format!("{} G-code moves mapped onto {:.1} × {:.1} mm bed; {} warnings",sim.steps.len(),bed_w,bed_h,sim.warnings.len()));
            if !sim.warnings.is_empty(){ui.colored_label(Color32::YELLOW,sim.warnings.iter().take(3).cloned().collect::<Vec<_>>().join(" | "));}
            ui.small("Camera alignment is approximate. Lens distortion, perspective, camera movement and workpiece height cause positioning errors. Recalibrate whenever the camera moves. Never trust this preview alone for cutting.");
        });
        self.open = open;
        if !self.open {
            self.live = false;
        }
    }
}
