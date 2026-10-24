//! Offline drag-and-drop editor for the bounded routine AST.
//! Drag palette blocks into any lane, or drag existing blocks to reorder/move them.
use crate::macro_routines::{Expr, Step};
use egui::{self, Color32, Sense};

#[derive(Clone, Debug)]
pub enum Source {
    Palette(usize),
    Existing(Vec<usize>),
}
#[derive(Clone, Debug)]
pub struct DragState {
    pub source: Source,
}

/// Editor-only history and clipboard; no device commands are issued.
#[derive(Default)]
pub struct CanvasEditorState {
    pub undo: Vec<Vec<Step>>,
    pub redo: Vec<Vec<Step>>,
    pub clipboard: Option<Step>,
    pub search: String,
    pub selected: Option<Vec<usize>>,
    pub paste_target: Vec<usize>,
    pub paste_index: usize,
    /// Explicit request, allowing the empty root lane to be a paste destination.
    pub paste_pending: bool,
    /// Active debugger source block, represented as the editor lane/index path.
    pub highlighted_path: Option<Vec<usize>>,
}
impl CanvasEditorState {
    pub fn commit(&mut self, before: Vec<Step>, after: &[Step]) {
        if format!("{:?}", before) != format!("{:?}", after) {
            self.undo.push(before);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    pub fn undo(&mut self, steps: &mut Vec<Step>) -> bool {
        if let Some(old) = self.undo.pop() {
            self.redo.push(std::mem::replace(steps, old));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self, steps: &mut Vec<Step>) -> bool {
        if let Some(new) = self.redo.pop() {
            self.undo.push(std::mem::replace(steps, new));
            true
        } else {
            false
        }
    }
    pub fn copy(&mut self, steps: &[Step], path: &[usize]) -> Result<(), String> {
        let (lane, i) = split_block(path).ok_or("Invalid path")?;
        let block = lane_ref(steps, lane).and_then(|v| v.get(i)).ok_or("Missing block")?;
        self.clipboard = Some(block.clone());
        Ok(())
    }
    pub fn paste(&mut self, steps: &mut Vec<Step>, lane: &[usize], index: usize) -> Result<(), String> {
        let block = self.clipboard.clone().ok_or("Clipboard empty")?;
        let mut candidate = steps.clone();
        let dest = lane_mut(&mut candidate, lane).ok_or("Invalid destination")?;
        if dest.len() >= 256 {
            return Err("Lane limit reached".into());
        }
        dest.insert(index.min(dest.len()), block);
        *steps = candidate;
        Ok(())
    }
}
fn lane_ref<'a>(root: &'a [Step], path: &[usize]) -> Option<&'a [Step]> {
    if path.is_empty() {
        return Some(root);
    }
    if path.len() < 2 {
        return None;
    }
    let (index, branch) = (path[0], path[1]);
    match root.get(index)? {
        Step::If { then_steps, else_steps, .. } => lane_ref(
            if branch == 0 {
                then_steps
            } else if branch == 1 {
                else_steps
            } else {
                return None;
            },
            &path[2..],
        ),
        Step::Repeat { steps, .. } if branch == 0 => lane_ref(steps, &path[2..]),
        _ => None,
    }
}

pub fn make_step(kind: usize) -> Step {
    match kind {
        0 => Step::SetOn { device: 0 },
        1 => Step::SetOff { device: 0 },
        2 => Step::SetPwm { device: 0, percent: 50 },
        3 => Step::WaitMs { ms: 1000 },
        4 => Step::If { condition: Expr::True, then_steps: vec![], else_steps: vec![] },
        5 => Step::Repeat { times: 2, steps: vec![] },
        6 => Step::Call { routine: String::new() },
        _ => Step::SetVariable { name: "value".into(), value: 0 },
    }
}
const NAMES: [&str; 8] = ["ON", "OFF", "PWM", "WAIT", "IF / ELSE", "REPEAT", "CALL", "VARIABLE"];

// Paths encode a block index followed by a branch: 0=THEN / REPEAT, 1=ELSE.
// A lane path ends at a branch selector; a block path ends at an index.
fn lane_mut<'a>(root: &'a mut Vec<Step>, path: &[usize]) -> Option<&'a mut Vec<Step>> {
    if path.is_empty() {
        return Some(root);
    }
    if path.len() < 2 {
        return None;
    }
    let (index, branch) = (path[0], path[1]);
    match root.get_mut(index)? {
        Step::If { then_steps, else_steps, .. } => lane_mut(
            if branch == 0 {
                then_steps
            } else if branch == 1 {
                else_steps
            } else {
                return None;
            },
            &path[2..],
        ),
        Step::Repeat { steps, .. } if branch == 0 => lane_mut(steps, &path[2..]),
        _ => None,
    }
}
fn split_block(path: &[usize]) -> Option<(&[usize], usize)> {
    Some((path.get(..path.len().checked_sub(1)?)?, *path.last()?))
}
fn take_block(root: &mut Vec<Step>, path: &[usize]) -> Option<Step> {
    let (lane, i) = split_block(path)?;
    let steps = lane_mut(root, lane)?;
    if i < steps.len() { Some(steps.remove(i)) } else { None }
}
fn is_prefix(a: &[usize], b: &[usize]) -> bool {
    b.starts_with(a)
}

/// Move or copy one block. Destination is a lane path plus insertion index.
/// Reject moving a parent inside itself, and adjust same-lane indices.
pub fn apply_drop(root: &mut Vec<Step>, source: Source, destination_lane: &[usize], mut at: usize) -> Result<(), String> {
    // Work on a copy: failed moves must never discard a block.
    let mut candidate = root.clone();
    match source {
        Source::Palette(kind) => {
            if kind >= NAMES.len() {
                return Err("Unknown palette block".into());
            }
            let target = lane_mut(&mut candidate, destination_lane).ok_or("Invalid destination")?;
            if target.len() >= 256 {
                return Err("Lane limit reached".into());
            }
            target.insert(at.min(target.len()), make_step(kind));
        }
        Source::Existing(path) => {
            let (src_lane, src_idx) = split_block(&path).ok_or("Invalid source")?;
            if is_prefix(&path, destination_lane) {
                return Err("Cannot move a block inside itself".into());
            }
            if lane_mut(&mut candidate, destination_lane).is_none() {
                return Err("Invalid destination".into());
            }
            let item = take_block(&mut candidate, &path).ok_or("Missing source block")?;
            let mut adjusted = destination_lane.to_vec();
            if src_lane == destination_lane && src_idx < at {
                at -= 1;
            }
            if destination_lane.starts_with(src_lane) && destination_lane.len() > src_lane.len() {
                let pos = src_lane.len();
                if adjusted[pos] > src_idx {
                    adjusted[pos] -= 1;
                }
            }
            let target = lane_mut(&mut candidate, &adjusted).ok_or("Destination changed")?;
            if target.len() >= 256 {
                return Err("Lane limit reached".into());
            }
            target.insert(at.min(target.len()), item);
        }
    }
    *root = candidate;
    Ok(())
}

pub fn duplicate_block(root: &mut Vec<Step>, path: &[usize]) -> Result<(), String> {
    let (lane, index) = split_block(path).ok_or("Invalid block path")?;
    let target = lane_mut(root, lane).ok_or("Invalid block lane")?;
    if target.len() >= 256 {
        return Err("Lane limit reached".into());
    }
    let block = target.get(index).ok_or("Missing block")?.clone();
    target.insert(index + 1, block);
    Ok(())
}

pub fn delete_block(root: &mut Vec<Step>, path: &[usize]) -> Result<(), String> {
    take_block(root, path).map(|_| ()).ok_or_else(|| "Missing block".into())
}

fn drop_target(ui: &mut egui::Ui, drag: &mut Option<DragState>, pending: &mut Option<(Source, Vec<usize>, usize)>, lane: &[usize], at: usize) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(80.0), 12.0), Sense::hover());
    let hovered = response.hovered();
    if hovered && drag.is_some() {
        ui.painter().line_segment([rect.left_center(), rect.right_center()], (2.0, Color32::LIGHT_BLUE));
    }
    if hovered
        && ui.input(|i| i.pointer.any_released())
        && let Some(d) = drag.take()
    {
        *pending = Some((d.source, lane.to_vec(), at));
    }
}
fn block_label(step: &Step) -> &'static str {
    match step {
        Step::SetOn { .. } => "ON",
        Step::SetOff { .. } => "OFF",
        Step::SetPwm { .. } => "PWM",
        Step::WaitMs { .. } => "WAIT",
        Step::If { .. } => "IF / ELSE",
        Step::Repeat { .. } => "REPEAT",
        Step::Call { .. } => "CALL",
        Step::SetVariable { .. } => "VARIABLE",
    }
}
#[allow(clippy::too_many_arguments)]
fn lane_ui(
    ui: &mut egui::Ui,
    steps: &mut [Step],
    lane: &[usize],
    drag: &mut Option<DragState>,
    pending: &mut Option<(Source, Vec<usize>, usize)>,
    edit: &mut Option<(bool, Vec<usize>)>,
    state: &mut CanvasEditorState,
    depth: usize,
) {
    if depth > 12 {
        ui.label("Maximum nesting reached");
        return;
    }
    for i in 0..=steps.len() {
        drop_target(ui, drag, pending, lane, i);
        ui.horizontal(|ui| {
            if ui.small_button("Paste here").on_hover_text("Insert clipboard block at this position").clicked() {
                state.paste_target = lane.to_vec();
                state.paste_index = i;
                state.paste_pending = true;
            }
        });
        if i == steps.len() {
            break;
        }
        let mut path = lane.to_vec();
        path.push(i);
        ui.push_id(("canvas", path.clone()), |ui| {
            let active = state.highlighted_path.as_ref() == Some(&path);
            let mut frame = egui::Frame::group(ui.style());
            if active {
                frame = frame.fill(Color32::from_rgb(45, 75, 105)).stroke(egui::Stroke::new(2.0, Color32::LIGHT_BLUE));
            }
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    let handle = ui.add(egui::Label::new("⠿ drag").sense(Sense::drag()));
                    if handle.drag_started() {
                        *drag = Some(DragState { source: Source::Existing(path.clone()) });
                    }
                    ui.strong(block_label(&steps[i]));
                    if ui.small_button("Copy").clicked() {
                        state.selected = Some(path.clone());
                    }
                    if ui.small_button("Duplicate").clicked() {
                        *edit = Some((true, path.clone()));
                    }
                    if ui.small_button("Delete").clicked() {
                        *edit = Some((false, path.clone()));
                    }
                    match &mut steps[i] {
                        Step::SetOn { device } | Step::SetOff { device } => {
                            ui.label("Device");
                            ui.add(egui::DragValue::new(device).range(0..=255));
                        }
                        Step::SetPwm { device, percent } => {
                            ui.label("Device");
                            ui.add(egui::DragValue::new(device).range(0..=255));
                            ui.add(egui::Slider::new(percent, 0..=100));
                        }
                        Step::WaitMs { ms } => {
                            ui.label("ms");
                            ui.add(egui::DragValue::new(ms).range(0..=60000));
                        }
                        Step::SetVariable { name, value } => {
                            ui.text_edit_singleline(name);
                            ui.add(egui::DragValue::new(value));
                        }
                        Step::Call { routine } => {
                            ui.text_edit_singleline(routine);
                        }
                        Step::Repeat { times, .. } => {
                            ui.add(egui::DragValue::new(times).range(0..=100));
                            ui.label("times");
                        }
                        Step::If { .. } => {}
                    }
                });
                match &mut steps[i] {
                    Step::Repeat { steps: body, .. } => {
                        let mut child = path.clone();
                        child.push(0);
                        ui.label("REPEAT body — drop blocks below");
                        ui.indent("repeat", |ui| lane_ui(ui, body, &child, drag, pending, edit, state, depth + 1));
                    }
                    Step::If { condition, then_steps, else_steps } => {
                        super::laser_panel::edit_condition(ui, condition, 0);
                        let mut then_lane = path.clone();
                        then_lane.push(0);
                        ui.label("THEN — drop blocks below");
                        ui.indent("then", |ui| lane_ui(ui, then_steps, &then_lane, drag, pending, edit, state, depth + 1));
                        let mut else_lane = path.clone();
                        else_lane.push(1);
                        ui.label("ELSE — drop blocks below");
                        ui.indent("else", |ui| lane_ui(ui, else_steps, &else_lane, drag, pending, edit, state, depth + 1));
                    }
                    _ => {}
                }
            });
        });
    }
}

pub fn show(ui: &mut egui::Ui, steps: &mut Vec<Step>, drag: &mut Option<DragState>, state: &mut CanvasEditorState) -> Option<String> {
    let before = steps.clone();
    let mut paste_requested = false;
    let mut undo_requested = false;
    let mut redo_requested = false;
    let shortcuts = ui.input(|i| {
        (
            i.modifiers.command && i.key_pressed(egui::Key::Z),
            i.modifiers.command && i.key_pressed(egui::Key::Y),
            i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z),
        )
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!state.undo.is_empty(), egui::Button::new("Undo")).clicked() {
            undo_requested = true;
        }
        if ui.add_enabled(!state.redo.is_empty(), egui::Button::new("Redo")).clicked() {
            redo_requested = true;
        }
        ui.label("Find block");
        ui.text_edit_singleline(&mut state.search);
        if ui.add_enabled(state.clipboard.is_some(), egui::Button::new("Paste at end")).clicked() {
            state.paste_target.clear();
            state.paste_index = steps.len();
            paste_requested = true;
        }
    });
    state.paste_pending = false;
    let mut pending = None;
    let mut edit = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("Drag a block into the canvas:");
        for (kind, name) in NAMES.iter().enumerate().filter(|(_, name)| name.to_ascii_lowercase().contains(&state.search.trim().to_ascii_lowercase()))
        {
            let response = ui.add(egui::Label::new(format!("⠿ {name}")).sense(Sense::drag()));
            if response.drag_started() {
                *drag = Some(DragState { source: Source::Palette(kind) });
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().max_height(530.0).show(ui, |ui| lane_ui(ui, steps, &[], drag, &mut pending, &mut edit, state, 0));
    if undo_requested || shortcuts.0 {
        state.undo(steps);
        return None;
    }
    if redo_requested || shortcuts.1 || shortcuts.2 {
        state.redo(steps);
        return None;
    }
    if paste_requested {
        let lane = state.paste_target.clone();
        let index = state.paste_index;
        let error = state.paste(steps, &lane, index).err();
        state.commit(before, steps);
        return error;
    }
    if let Some(path) = state.selected.take() {
        // "Copy" selects an existing block; "Paste after" selects an insertion position.
        if let Err(e) = state.copy(steps, &path) {
            return Some(e);
        }
    }
    if state.paste_pending {
        state.paste_pending = false;
        let lane = std::mem::take(&mut state.paste_target);
        let index = state.paste_index;
        let error = state.paste(steps, &lane, index).err();
        state.commit(before, steps);
        return error;
    }
    if let Some((source, lane, at)) = pending {
        let result = apply_drop(steps, source, &lane, at).err();
        state.commit(before, steps);
        return result;
    }
    if let Some((copy, path)) = edit {
        let result = if copy { duplicate_block(steps, &path) } else { delete_block(steps, &path) }.err();
        state.commit(before, steps);
        return result;
    }
    if ui.input(|i| i.pointer.any_released()) {
        *drag = None;
    }
    if drag.is_some() {
        ui.ctx().request_repaint();
    }
    state.commit(before, steps);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_into_nested_then() {
        let mut r = vec![make_step(4)];
        apply_drop(&mut r, Source::Palette(2), &[0, 0], 0).unwrap();
        assert!(matches!(&r[0],Step::If{then_steps,..} if matches!(then_steps[0],Step::SetPwm{..})));
    }
    #[test]
    fn reorder_same_lane() {
        let mut r = vec![make_step(0), make_step(1), make_step(3)];
        apply_drop(&mut r, Source::Existing(vec![0]), &[], 3).unwrap();
        assert!(matches!(r[2], Step::SetOn { .. }));
    }
    #[test]
    fn failed_move_preserves_source() {
        let mut r = vec![make_step(0), make_step(4)];
        let before = format!("{:?}", r);
        assert!(apply_drop(&mut r, Source::Existing(vec![0]), &[1, 0, 3], 0).is_err());
        assert_eq!(format!("{:?}", r), before);
    }
    #[test]
    fn duplicate_and_delete_nested() {
        let mut r = vec![make_step(4)];
        apply_drop(&mut r, Source::Palette(0), &[0, 0], 0).unwrap();
        duplicate_block(&mut r, &[0, 0, 0]).unwrap();
        if let Step::If { then_steps, .. } = &r[0] {
            assert_eq!(then_steps.len(), 2)
        } else {
            panic!()
        };
        delete_block(&mut r, &[0, 0, 1]).unwrap();
        if let Step::If { then_steps, .. } = &r[0] {
            assert_eq!(then_steps.len(), 1)
        } else {
            panic!()
        };
    }
    #[test]
    fn reject_self_nesting() {
        let mut r = vec![make_step(4)];
        assert!(apply_drop(&mut r, Source::Existing(vec![0]), &[0, 0], 0).is_err());
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn undo_redo_and_branch_invalidation() {
        let mut editor = CanvasEditorState::default();
        let mut steps = vec![make_step(0)];
        let old = steps.clone();
        steps.push(make_step(1));
        editor.commit(old, &steps);
        assert!(editor.undo(&mut steps));
        assert_eq!(steps.len(), 1);
        assert!(editor.redo(&mut steps));
        assert_eq!(steps.len(), 2);
        assert!(editor.undo(&mut steps));
        let old = steps.clone();
        steps.push(make_step(2));
        editor.commit(old, &steps);
        assert!(!editor.redo(&mut steps));
    }
    #[test]
    fn paste_at_nested_position() {
        let mut editor = CanvasEditorState::default();
        let mut steps = vec![make_step(4)];
        editor.clipboard = Some(make_step(2));
        editor.paste(&mut steps, &[0, 1], 0).unwrap();
        assert!(matches!(&steps[0],Step::If{else_steps,..} if matches!(else_steps[0],Step::SetPwm{..})));
    }
    #[test]
    fn paste_into_root_lane() {
        let mut editor = CanvasEditorState::default();
        let mut steps = vec![make_step(0)];
        editor.clipboard = Some(make_step(3));
        editor.paste(&mut steps, &[], 0).unwrap();
        assert!(matches!(steps[0], Step::WaitMs { .. }));
    }
    #[test]
    fn clipboard_copies_nested_block() {
        let mut editor = CanvasEditorState::default();
        let mut steps = vec![make_step(4)];
        apply_drop(&mut steps, Source::Palette(3), &[0, 0], 0).unwrap();
        editor.copy(&steps, &[0, 0, 0]).unwrap();
        editor.paste(&mut steps, &[], 1).unwrap();
        assert!(matches!(steps[1], Step::WaitMs { .. }));
    }
}
