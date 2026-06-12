use std::path::PathBuf;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Layout, Margin, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Vec2,
};

use rtv_save_core::catalog::{self, Catalog};
use rtv_save_core::character::{EQUIPMENT_SLOTS, FLAG_FIELDS, STAT_FIELDS};
use rtv_save_core::edit;
use rtv_save_core::tres::Value;
use rtv_save_core::validate::Severity;
use rtv_save_core::{load, repair_document, save_with_backup, Document, Report};

// --- palette ---------------------------------------------------------------
const BG: Color32 = Color32::from_rgb(0x15, 0x18, 0x15);
const CARD_BG: Color32 = Color32::from_rgb(0x21, 0x25, 0x21);
const CARD_STROKE: Color32 = Color32::from_rgb(0x35, 0x3b, 0x33);
const ACCENT: Color32 = Color32::from_rgb(0xCB, 0x9B, 0x3C); // amber
const ACCENT2: Color32 = Color32::from_rgb(0x93, 0xAC, 0x5A); // olive
const TEXT: Color32 = Color32::from_rgb(0xD6, 0xD8, 0xCE);

#[derive(PartialEq, Eq, Clone, Copy)]
enum View {
    Character,
    Diagnostics,
}

pub struct EditorApp {
    project_path: String,
    catalog: Catalog,
    file_path: Option<PathBuf>,
    doc: Option<Document>,
    report: Option<Report>,
    repair_log: Vec<String>,
    view: View,
    /// Which grid container the centre panel shows ("inventory" or "catalog").
    container: &'static str,
    status: String,
    dirty: bool,
    add_search: String,
    selected: Option<(String, String)>,
    drag: Option<DragState>,
    equip_target: Option<String>,
    themed: bool,
}

struct DragState {
    sub_id: String,
    grab: Vec2,
}

impl EditorApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let project_path = "X:/RTVReversed".to_string();
        let catalog = catalog::scan(std::path::Path::new(&project_path));
        let status = if catalog.items.is_empty() {
            "Item catalog empty - set the project path and click Rescan.".to_string()
        } else {
            format!("Loaded {} items.", catalog.items.len())
        };
        Self {
            project_path,
            catalog,
            file_path: None,
            doc: None,
            report: None,
            repair_log: Vec::new(),
            view: View::Character,
            container: "inventory",
            status,
            dirty: false,
            add_search: String::new(),
            selected: None,
            drag: None,
            equip_target: None,
            themed: false,
        }
    }

    // ----- file actions ----------------------------------------------------

    fn open_path(&mut self, path: PathBuf) {
        match load(&path) {
            Ok((doc, report)) => {
                let errs = report.errors();
                self.view = if errs > 0 { View::Diagnostics } else { View::Character };
                self.status = format!("Opened {} - {} error(s), {} warning(s).", path.display(), errs, report.warnings());
                self.doc = Some(doc);
                self.report = Some(report);
                self.file_path = Some(path);
                self.repair_log.clear();
                self.selected = None;
                self.dirty = false;
            }
            Err(e) => self.status = format!("Failed to open: {}", e),
        }
    }

    fn revalidate(&mut self) {
        if let Some(doc) = &self.doc {
            let text = doc.to_tres();
            let (newdoc, report) = rtv_save_core::validate::validate(&text);
            self.doc = Some(newdoc);
            self.report = Some(report);
        }
    }

    fn do_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Godot resource", &["tres"])
            .set_file_name("Character.tres")
            .pick_file()
        {
            self.open_path(path);
        }
    }

    fn do_save(&mut self) {
        let Some(doc) = &self.doc else { return };
        let Some(path) = self.file_path.clone() else {
            self.do_save_as();
            return;
        };
        match save_with_backup(doc, &path) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Saved {} (backup -> *.tres.bak).", path.display());
            }
            Err(e) => self.status = format!("Save failed: {}", e),
        }
    }

    fn do_save_as(&mut self) {
        let Some(doc) = &self.doc else { return };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Godot resource", &["tres"])
            .set_file_name("Character.tres")
            .save_file()
        {
            match save_with_backup(doc, &path) {
                Ok(()) => {
                    self.file_path = Some(path.clone());
                    self.dirty = false;
                    self.status = format!("Saved {}.", path.display());
                }
                Err(e) => self.status = format!("Save failed: {}", e),
            }
        }
    }

    fn do_repair(&mut self) {
        let Some(mut doc) = self.doc.take() else { return };
        let log = repair_document(&mut doc, &self.catalog);
        self.repair_log = log.actions;
        self.doc = Some(doc);
        self.revalidate();
        self.dirty = true;
        let ok = self.report.as_ref().map(|r| r.is_ok()).unwrap_or(false);
        self.status = if ok {
            "Repair complete - file is now valid. Review and Save.".into()
        } else {
            "Repair ran, but some issues remain (see Diagnostics).".into()
        };
    }

    fn rescan_catalog(&mut self) {
        self.catalog = catalog::scan(std::path::Path::new(&self.project_path));
        self.status = format!("Rescanned items: {} found.", self.catalog.items.len());
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.themed {
            setup_theme(ctx);
            self.themed = true;
        }

        self.top_bar(ctx);
        self.status_bar(ctx);

        if self.doc.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(140.0);
                    ui.label(RichText::new("RtV SAVE EDITOR").color(ACCENT).size(28.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Open a Road to Vostok Character.tres to begin.").color(TEXT));
                    ui.add_space(16.0);
                    if ui.button(RichText::new("Open Character.tres").size(15.0)).clicked() {
                        self.do_open();
                    }
                });
            });
            return;
        }

        match self.view {
            View::Character => {
                egui::SidePanel::left("left")
                    .exact_width(312.0)
                    .resizable(false)
                    .show(ctx, |ui| self.left_panel(ui));
                egui::SidePanel::right("right")
                    .exact_width(336.0)
                    .resizable(false)
                    .show(ctx, |ui| self.right_panel(ui));
                egui::CentralPanel::default().show(ctx, |ui| self.center_panel(ui));
            }
            View::Diagnostics => {
                egui::CentralPanel::default().show(ctx, |ui| self.diagnostics_tab(ui));
            }
        }
    }
}

//status bars

impl EditorApp {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(2.0);
            egui::menu::bar(ui, |ui| {
                ui.label(RichText::new("RtV Save Editor").color(ACCENT).strong().size(16.0));
                ui.separator();
                if ui.button("Open").clicked() {
                    self.do_open();
                }
                let has = self.doc.is_some();
                ui.add_enabled_ui(has, |ui| {
                    if ui.button("Save").clicked() {
                        self.do_save();
                    }
                    if ui.button("Save As").clicked() {
                        self.do_save_as();
                    }
                });
                ui.separator();
                ui.label(RichText::new("project").weak());
                ui.add(egui::TextEdit::singleline(&mut self.project_path).desired_width(180.0));
                if ui.button("Rescan").on_hover_text("Rescan item catalog").clicked() {
                    self.rescan_catalog();
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let errs = self.report.as_ref().map(|r| r.errors()).unwrap_or(0);
                    let diag = if errs > 0 {
                        RichText::new(format!("Diagnostics ({})", errs)).color(Color32::from_rgb(225, 110, 110))
                    } else {
                        RichText::new("Diagnostics")
                    };
                    ui.selectable_value(&mut self.view, View::Diagnostics, diag);
                    ui.selectable_value(&mut self.view, View::Character, RichText::new("Character").strong());
                    if self.dirty {
                        ui.colored_label(ACCENT, "unsaved");
                    }
                });
            });
            ui.add_space(2.0);
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(r) = &self.report {
                    let (c, t) = if r.errors() > 0 {
                        (Color32::from_rgb(225, 110, 110), format!("{} error(s)", r.errors()))
                    } else if r.warnings() > 0 {
                        (ACCENT, format!("{} warning(s)", r.warnings()))
                    } else {
                        (ACCENT2, "valid".to_string())
                    };
                    ui.colored_label(c, t);
                    ui.separator();
                }
                ui.label(RichText::new(&self.status).weak());
            });
        });
    }
}

impl EditorApp {
    fn left_panel(&mut self, ui: &mut egui::Ui) {
        if self.doc.as_ref().and_then(|d| d.resource()).is_none() {
            no_resource_notice(ui);
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            card(ui, "Vitals", |ui| self.vitals_inner(ui));
            ui.add_space(8.0);
            card(ui, "Conditions", |ui| self.status_inner(ui));
            ui.add_space(8.0);
            card(ui, "Equipment", |ui| self.equipment_inner(ui));
        });
    }

    fn vitals_inner(&mut self, ui: &mut egui::Ui) {
        for (name, _) in STAT_FIELDS {
            let cur = self
                .doc
                .as_ref()
                .and_then(|d| d.resource())
                .and_then(|r| r.value(name))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            if let Some(nv) = stat_bar(ui, name, cur) {
                if let Some(r) = self.doc.as_mut().and_then(|d| d.resource_mut()) {
                    r.set(name, Value::Float(nv));
                    self.dirty = true;
                }
            }
            ui.add_space(4.0);
        }
    }

    fn status_inner(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (name, _) in FLAG_FIELDS {
                // Equip flags are derived from the Equipment panel; skip the noise.
                if matches!(*name, "primary" | "secondary" | "knife" | "grenade1" | "grenade2" | "flashlight" | "NVG") {
                    continue;
                }
                let cur = self
                    .doc
                    .as_ref()
                    .and_then(|d| d.resource())
                    .and_then(|r| r.value(name))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let chip = RichText::new(*name).size(12.0).color(if cur { Color32::BLACK } else { TEXT });
                let resp = ui.add(egui::Button::new(chip).fill(if cur { ACCENT } else { Color32::from_gray(40) }).min_size(Vec2::new(0.0, 22.0)));
                if resp.clicked() {
                    if let Some(r) = self.doc.as_mut().and_then(|d| d.resource_mut()) {
                        r.set(name, Value::Bool(!cur));
                        self.dirty = true;
                    }
                }
            }
        });
    }

    fn equipment_inner(&mut self, ui: &mut egui::Ui) {
        let equip = {
            let doc = self.doc.as_ref().unwrap();
            edit::list_slots(doc, "equipment", &self.catalog)
        };
        const GROUPS: &[(&str, &[&str])] = &[
            ("Weapons", &["Primary", "Secondary", "Knife"]),
            ("Throwables", &["Grenade_1", "Grenade_2"]),
            ("Clothing", &["Helmet", "Head", "Torso", "Legs", "Feet", "Hands"]),
            ("Carry", &["Backpack", "Rig", "Belt"]),
            ("Gadgets", &["Matches", "Light", "NVG"]),
        ];
        for (group, slots) in GROUPS {
            ui.label(RichText::new(*group).color(ACCENT2).size(11.0).strong());
            for name in *slots {
                let found = equip.iter().find(|s| &s.slot == name);
                let is_sel = matches!((&self.selected, found),
                    (Some((a, s)), Some(f)) if a == "equipment" && s == &f.sub_id);
                if self.slot_row(ui, name, found, is_sel) {
                    match found {
                        Some(f) => {
                            self.selected = Some(("equipment".into(), f.sub_id.clone()));
                            self.equip_target = None;
                        }
                        None => self.equip_target = Some(name.to_string()),
                    }
                }
            }
            ui.add_space(4.0);
        }
        // Slots present in the array but not in the standard list (safety).
        for s in &equip {
            if !EQUIPMENT_SLOTS.contains(&s.slot.as_str()) && !s.slot.is_empty() {
                let is_sel = matches!(&self.selected, Some((a, id)) if a == "equipment" && id == &s.sub_id);
                if self.slot_row(ui, &s.slot, Some(s), is_sel) {
                    self.selected = Some(("equipment".into(), s.sub_id.clone()));
                }
            }
        }
    }

    /// One equipment slot row; returns true if clicked.
    fn slot_row(&self, ui: &mut egui::Ui, slot: &str, item: Option<&edit::SlotView>, selected: bool) -> bool {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
        let painter = ui.painter();
        let bg = if selected {
            ACCENT.gamma_multiply(0.25)
        } else if resp.hovered() {
            Color32::from_gray(46)
        } else {
            Color32::from_gray(36)
        };
        painter.rect_filled(rect, CornerRadius::same(5), bg);
        if selected {
            painter.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.5, ACCENT), StrokeKind::Inside);
        }
        // category chip
        let chip = Rect::from_min_size(rect.min + Vec2::new(5.0, 5.0), Vec2::splat(16.0));
        let chip_col = item.map(|s| category_color(&s.item_path)).unwrap_or(Color32::from_gray(55));
        painter.rect_filled(chip, CornerRadius::same(3), chip_col);
        painter.text(rect.left_center() + Vec2::new(28.0, 0.0), Align2::LEFT_CENTER, slot, FontId::proportional(11.0), Color32::from_gray(170));
        let name = item.map(|s| s.item_name.as_str()).unwrap_or("-");
        painter.text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, name, FontId::proportional(12.0), TEXT);
        resp.clicked()
    }
}

//grid ui
impl EditorApp {
    fn center_panel(&mut self, ui: &mut egui::Ui) {
        if self.doc.as_ref().and_then(|d| d.resource()).is_none() {
            no_resource_notice(ui);
            return;
        }
        let container = self.container;
        if let Some((arr, id)) = self.selected.clone() {
            if arr == container && ui.input(|i| i.key_pressed(egui::Key::R)) {
                if let Some(doc) = self.doc.as_mut() {
                    if edit::rotate_item(doc, container, &self.catalog, &id) {
                        self.dirty = true;
                    }
                }
            }
        }

        let count = {
            let doc = self.doc.as_ref().unwrap();
            edit::list_slots(doc, container, &self.catalog).len()
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.container, "inventory", RichText::new("  Inventory  ").size(14.0));
            ui.selectable_value(&mut self.container, "catalog", RichText::new("  Stash  ").size(14.0));
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Re-pack").on_hover_text("Tidy the grid layout").clicked() {
                    if let Some(doc) = self.doc.as_mut() {
                        edit::repack(doc, self.container, &self.catalog);
                        self.dirty = true;
                    }
                }
                ui.label(RichText::new(format!("{} items", count)).weak());
            });
        });
        ui.label(RichText::new("drag to move - R rotates the selected item").weak().small());
        ui.add_space(6.0);

        ui.vertical_centered(|ui| {
            self.draw_grid(ui, self.container);
        });
    }

    fn draw_grid(&mut self, ui: &mut egui::Ui, container: &str) {
        let cs = 46.0_f32;
        let gw = edit::GRID_W;
        let gh = edit::GRID_H;
        let size = Vec2::new(gw as f32 * cs, gh as f32 * cs);
        let (resp, painter) = ui.allocate_painter(size, Sense::hover());
        let origin = resp.rect.min;

        // board backdrop
        painter.rect_filled(Rect::from_min_size(origin, size).expand(6.0), CornerRadius::same(8), Color32::from_gray(18));
        for y in 0..gh {
            for x in 0..gw {
                let r = Rect::from_min_size(origin + Vec2::new(x as f32 * cs, y as f32 * cs), Vec2::splat(cs)).shrink(1.0);
                painter.rect_filled(r, CornerRadius::same(3), Color32::from_gray(30));
                painter.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, Color32::from_gray(48)), StrokeKind::Inside);
            }
        }

        let slots = {
            let doc = self.doc.as_ref().unwrap();
            edit::list_slots(doc, container, &self.catalog)
        };
        let pointer = ui.ctx().pointer_interact_pos();
        let mut commit: Option<(String, i64, i64)> = None;
        let mut any_dragging = false;

        for slot in &slots {
            let (w, h) = edit::effective_size(&self.catalog, &slot.item_path, slot.rotated);
            let item_origin = origin + Vec2::new(slot.grid_x as f32 * cs, slot.grid_y as f32 * cs);
            let item_rect = Rect::from_min_size(item_origin, Vec2::new(w as f32 * cs, h as f32 * cs)).shrink(2.5);
            let id = egui::Id::new(("griditem", container, &slot.sub_id));
            let r = ui.interact(item_rect, id, Sense::click_and_drag());

            let is_sel = matches!(&self.selected, Some((a, s)) if a == container && s == &slot.sub_id);
            let is_dragged = self.drag.as_ref().map(|d| d.sub_id == slot.sub_id).unwrap_or(false);

            let base = category_color(&slot.item_path);
            let fill = if is_dragged { base.gamma_multiply(0.3) } else { base };
            painter.rect_filled(item_rect, CornerRadius::same(5), fill);
            // top sheen
            let sheen = Rect::from_min_size(item_rect.min, Vec2::new(item_rect.width(), item_rect.height() * 0.4));
            painter.rect_filled(sheen, CornerRadius::same(5), Color32::from_white_alpha(10));
            let (border, bw) = if is_sel { (ACCENT, 2.5) } else { (Color32::from_gray(14), 1.0) };
            painter.rect_stroke(item_rect, CornerRadius::same(5), Stroke::new(bw, border), StrokeKind::Inside);
            painter.text(item_rect.min + Vec2::new(5.0, 4.0), Align2::LEFT_TOP, &slot.item_name, FontId::proportional(11.0), text_on(fill));
            if slot.amount > 1 {
                painter.text(item_rect.max - Vec2::new(4.0, 3.0), Align2::RIGHT_BOTTOM, format!("x{}", slot.amount), FontId::proportional(11.0), Color32::WHITE);
            }
            if slot.condition < 100 {
                let frac = (slot.condition as f32 / 100.0).clamp(0.0, 1.0);
                let bar = Rect::from_min_size(Pos2::new(item_rect.min.x, item_rect.max.y - 3.0), Vec2::new(item_rect.width() * frac, 3.0));
                painter.rect_filled(bar, CornerRadius::same(0), cond_color(frac));
            }

            if r.clicked() {
                self.selected = Some((container.to_string(), slot.sub_id.clone()));
            }
            if r.drag_started() {
                let grab = pointer.map(|p| p - item_rect.min).unwrap_or(Vec2::ZERO);
                self.drag = Some(DragState { sub_id: slot.sub_id.clone(), grab });
                self.selected = Some((container.to_string(), slot.sub_id.clone()));
            }
            if is_dragged && (r.dragged() || r.drag_stopped()) {
                any_dragging = true;
                if let (Some(p), Some(d)) = (pointer, self.drag.as_ref()) {
                    let tl = p - d.grab;
                    let max_x = (gw as i64 - w as i64).max(0);
                    let max_y = (gh as i64 - h as i64).max(0);
                    let gx = (((tl.x - origin.x) / cs).round() as i64).clamp(0, max_x);
                    let gy = (((tl.y - origin.y) / cs).round() as i64).clamp(0, max_y);
                    let ok = {
                        let doc = self.doc.as_ref().unwrap();
                        edit::can_place_at(doc, container, &self.catalog, &slot.sub_id, gx, gy)
                    };
                    let tgt = Rect::from_min_size(origin + Vec2::new(gx as f32 * cs, gy as f32 * cs), Vec2::new(w as f32 * cs, h as f32 * cs)).shrink(2.5);
                    let col = if ok { ACCENT2 } else { Color32::from_rgb(210, 80, 80) };
                    painter.rect_stroke(tgt, CornerRadius::same(5), Stroke::new(2.5, col), StrokeKind::Inside);
                    let ghost = Rect::from_min_size(p - d.grab, Vec2::new(w as f32 * cs, h as f32 * cs)).shrink(2.5);
                    painter.rect_filled(ghost, CornerRadius::same(5), base.gamma_multiply(0.55));
                    if r.drag_stopped() && ok {
                        commit = Some((slot.sub_id.clone(), gx, gy));
                    }
                }
            }
        }

        if let Some((id, gx, gy)) = commit {
            if let Some(doc) = self.doc.as_mut() {
                edit::move_item(doc, container, &self.catalog, &id, gx, gy);
                self.dirty = true;
            }
            self.drag = None;
        }
        if !any_dragging && self.drag.is_some() && !ui.ctx().input(|i| i.pointer.any_down()) {
            self.drag = None;
        }
    }
}


//character 
impl EditorApp {
    fn right_panel(&mut self, ui: &mut egui::Ui) {
        if self.doc.as_ref().and_then(|d| d.resource()).is_none() {
            no_resource_notice(ui);
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.equip_target.is_some() {
                card(ui, "Equip slot", |ui| self.equip_add_inner(ui));
                ui.add_space(8.0);
            }
            card(ui, "Selected", |ui| self.selected_inner(ui));
            ui.add_space(8.0);
            let title = if self.container == "catalog" { "Add to stash" } else { "Add to inventory" };
            card(ui, title, |ui| self.add_inner(ui));
        });
    }

    fn selected_inner(&mut self, ui: &mut egui::Ui) {
        let Some((arr, id)) = self.selected.clone() else {
            ui.label(RichText::new("Click an item to edit it.").weak());
            return;
        };
        let slots = {
            let doc = self.doc.as_ref().unwrap();
            edit::list_slots(doc, &arr, &self.catalog)
        };
        let Some(s) = slots.iter().find(|x| x.sub_id == id).cloned() else {
            ui.label(RichText::new("(item no longer present)").weak());
            return;
        };

        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(4), category_color(&s.item_path));
            ui.label(RichText::new(&s.item_name).size(16.0).strong());
        });
        ui.label(RichText::new(&s.item_path).weak().small());
        if !s.slot.is_empty() {
            ui.label(format!("Equipped: {}", s.slot));
        } else {
            ui.label(format!("Cell {},{}{}", s.grid_x, s.grid_y, if s.rotated { "  (rotated)" } else { "" }));
        }
        if !s.nested.is_empty() {
            ui.label(RichText::new(format!("Contains: {}", s.nested.join(", "))).color(ACCENT2));
        }
        ui.add_space(8.0);

        let mut cond = s.condition;
        ui.horizontal(|ui| {
            ui.label("Condition");
            if ui.add(egui::DragValue::new(&mut cond).range(0..=100)).changed() {
                if let Some(doc) = self.doc.as_mut() {
                    edit::set_slot_field(doc, &id, "condition", Value::Int(cond));
                    self.dirty = true;
                }
            }
        });
        let mut amt = s.amount;
        ui.horizontal(|ui| {
            ui.label("Amount   ");
            if ui.add(egui::DragValue::new(&mut amt).range(0..=9999)).changed() {
                if let Some(doc) = self.doc.as_mut() {
                    edit::set_slot_field(doc, &id, "amount", Value::Int(amt));
                    self.dirty = true;
                }
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if arr == "equipment" {
                if ui.button("Change").on_hover_text("Replace this slot with another item").clicked() {
                    self.equip_target = Some(s.slot.clone());
                }
                if ui.button("Unequip").on_hover_text("Move it to the inventory").clicked() {
                    if let Some(doc) = self.doc.as_mut() {
                        edit::unequip(doc, &self.catalog, &id);
                        self.dirty = true;
                    }
                    self.selected = None;
                }
            } else if ui.button("Rotate").clicked() {
                if let Some(doc) = self.doc.as_mut() {
                    if edit::rotate_item(doc, &arr, &self.catalog, &id) {
                        self.dirty = true;
                    } else {
                        self.status = "No room to rotate.".into();
                    }
                }
            }
            if ui.button(RichText::new("Remove").color(Color32::from_rgb(230, 120, 120))).clicked() {
                if let Some(doc) = self.doc.as_mut() {
                    if arr == "equipment" {
                        edit::remove_equipment(doc, &id);
                    } else {
                        edit::remove_slot(doc, &arr, &id);
                    }
                    self.dirty = true;
                }
                self.selected = None;
            }
        });
    }

    fn add_inner(&mut self, ui: &mut egui::Ui) {
        if self.catalog.items.is_empty() {
            ui.label(RichText::new("No item catalog - set the project path in the toolbar and Rescan.").weak());
            return;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.add_search).desired_width(230.0).hint_text("search items..."));
            if !self.add_search.is_empty() && ui.button("clear").clicked() {
                self.add_search.clear();
            }
        });
        ui.add_space(4.0);

        let needle = self.add_search.to_lowercase();
        let target = self.container;
        let mut pending: Option<String> = None;
        egui::ScrollArea::vertical().max_height(360.0).id_salt("addlist").show(ui, |ui| {
            for item in &self.catalog.items {
                if !needle.is_empty()
                    && !item.display_name.to_lowercase().contains(&needle)
                    && !item.res_path.to_lowercase().contains(&needle)
                {
                    continue;
                }
                let resp = ui.allocate_response(Vec2::new(ui.available_width(), 24.0), Sense::click());
                let painter = ui.painter();
                let rect = resp.rect;
                if resp.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(4), Color32::from_gray(44));
                }
                let chip = Rect::from_min_size(rect.min + Vec2::new(2.0, 4.0), Vec2::splat(16.0));
                painter.rect_filled(chip, CornerRadius::same(3), category_color(&item.res_path));
                painter.text(rect.left_center() + Vec2::new(24.0, 0.0), Align2::LEFT_CENTER, &item.display_name, FontId::proportional(12.5), TEXT);
                let meta = if item.is_equippable() {
                    item.slots.join("/")
                } else {
                    format!("{}x{}", item.size.0, item.size.1)
                };
                painter.text(rect.right_center() - Vec2::new(6.0, 0.0), Align2::RIGHT_CENTER, meta, FontId::proportional(11.0), Color32::from_gray(150));
                if resp.clicked() {
                    pending = Some(item.res_path.clone());
                }
            }
        });

        if let Some(path) = pending {
            if let Some(doc) = self.doc.as_mut() {
                let new_id = edit::add_item(doc, &self.catalog, target, &path, None);
                self.dirty = true;
                if let Some(id) = new_id {
                    self.selected = Some((target.to_string(), id));
                    self.status = format!("Added {}.", self.catalog.name_of(&path));
                }
            }
        }
    }

    fn equip_add_inner(&mut self, ui: &mut egui::Ui) {
        let Some(slot) = self.equip_target.clone() else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Equip: {}", slot)).color(ACCENT).strong());
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("cancel").clicked() {
                    self.equip_target = None;
                }
            });
        });
        let mut pick: Option<String> = None;
        egui::ScrollArea::vertical().max_height(170.0).id_salt("equippick").show(ui, |ui| {
            let mut any = false;
            for item in &self.catalog.items {
                if !item.slots.iter().any(|s| s == &slot) {
                    continue;
                }
                any = true;
                if ui.button(RichText::new(&item.display_name).size(12.5)).clicked() {
                    pick = Some(item.res_path.clone());
                }
            }
            if !any {
                ui.label(RichText::new("(no catalog items fit this slot)").weak());
            }
        });
        if let Some(path) = pick {
            if let Some(doc) = self.doc.as_mut() {
                let new_id = edit::equip(doc, &self.catalog, &slot, &path);
                self.dirty = true;
                if let Some(id) = new_id {
                    self.selected = Some(("equipment".into(), id));
                }
            }
            self.equip_target = None;
        }
    }
}

//diag

impl EditorApp {
    fn diagnostics_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("DIAGNOSTICS").color(ACCENT).strong().size(18.0));
            let repairable = self.report.as_ref().map(|r| r.has_repairable()).unwrap_or(false);
            ui.add_enabled_ui(repairable, |ui| {
                if ui.button(RichText::new("Auto-repair").color(ACCENT2).strong()).clicked() {
                    self.do_repair();
                }
            });
            if ui.button("Re-validate").clicked() {
                self.revalidate();
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            match &self.report {
                Some(r) if r.diagnostics.is_empty() => {
                    ui.colored_label(ACCENT2, "No problems found - save is structurally valid.");
                }
                Some(r) => {
                    for d in &r.diagnostics {
                        let (color, tag) = match d.severity {
                            Severity::Error => (Color32::from_rgb(225, 110, 110), "ERROR"),
                            Severity::Warning => (ACCENT, "WARN "),
                            Severity::Info => (Color32::from_rgb(150, 180, 220), "INFO "),
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.colored_label(color, RichText::new(tag).monospace().strong());
                            if let Some(l) = d.line {
                                ui.label(RichText::new(format!("line {}", l)).weak());
                            }
                            ui.label(&d.message);
                            if d.repairable {
                                ui.label(RichText::new("(auto-repairable)").color(ACCENT2));
                            }
                        });
                    }
                }
                None => {}
            }

            if !self.repair_log.is_empty() {
                ui.add_space(12.0);
                ui.separator();
                ui.label(RichText::new("REPAIR LOG").color(ACCENT).strong());
                for line in &self.repair_log {
                    ui.label(RichText::new(format!("- {}", line)).monospace().size(12.0));
                }
            }
        });
    }
}

//helpers

fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) {
    egui::Frame::default()
        .fill(CARD_BG)
        .stroke(Stroke::new(1.0, CARD_STROKE))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title.to_uppercase()).color(ACCENT).strong().size(12.0));
            ui.add_space(6.0);
            add(ui);
        });
}

/// A horizontal vitals bar that doubles as a slider. Returns the new value
/// when the user drags/clicks it.
fn stat_bar(ui: &mut egui::Ui, label: &str, value: f64) -> Option<f64> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click_and_drag());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(5), Color32::from_gray(22));
    let frac = (value / 100.0).clamp(0.0, 1.0) as f32;
    if frac > 0.0 {
        let fill = Rect::from_min_size(rect.min, Vec2::new(rect.width() * frac, rect.height()));
        painter.rect_filled(fill, CornerRadius::same(5), stat_color(frac));
    }
    painter.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, Color32::from_gray(45)), StrokeKind::Inside);
    painter.text(rect.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), Color32::from_rgb(20, 22, 18));
    painter.text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, format!("{:.0}", value), FontId::proportional(12.0), Color32::WHITE);

    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let nv = (((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * 100.0) as f64;
            return Some((nv * 10.0).round() / 10.0);
        }
    }
    None
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn stat_color(frac: f32) -> Color32 {
    let red = Color32::from_rgb(190, 70, 55);
    let amber = Color32::from_rgb(205, 160, 60);
    let green = Color32::from_rgb(120, 175, 80);
    if frac < 0.5 {
        lerp(red, amber, frac / 0.5)
    } else {
        lerp(amber, green, (frac - 0.5) / 0.5)
    }
}

fn category_color(path: &str) -> Color32 {
    if path.contains("/Weapons/") {
        Color32::from_rgb(72, 98, 134)
    } else if path.contains("/Ammo/") {
        Color32::from_rgb(150, 102, 52)
    } else if path.contains("/Consumables/") {
        Color32::from_rgb(78, 124, 74)
    } else if path.contains("/Medical/") {
        Color32::from_rgb(142, 72, 82)
    } else if path.contains("/Clothing/") {
        Color32::from_rgb(64, 112, 112)
    } else if path.contains("/Electronics/") {
        Color32::from_rgb(108, 84, 134)
    } else if path.contains("/Belts/") {
        Color32::from_rgb(112, 92, 62)
    } else if path.contains("/Fishing/") {
        Color32::from_rgb(62, 112, 132)
    } else if path.contains("/Attachments/") {
        Color32::from_rgb(92, 92, 108)
    } else {
        Color32::from_gray(86)
    }
}

fn text_on(bg: Color32) -> Color32 {
    let lum = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if lum > 140.0 {
        Color32::BLACK
    } else {
        Color32::from_gray(238)
    }
}

fn cond_color(frac: f32) -> Color32 {
    lerp(Color32::from_rgb(200, 70, 60), Color32::from_rgb(110, 190, 90), frac)
}

fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = Color32::from_rgb(0x0e, 0x10, 0x0e);
    v.faint_bg_color = Color32::from_rgb(0x20, 0x24, 0x20);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = CARD_BG;
    v.widgets.noninteractive.weak_bg_fill = CARD_BG;
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x2c, 0x31, 0x2c);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x26, 0x2b, 0x26);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x39, 0x40, 0x39);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x39, 0x40, 0x39);
    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.55);
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.window_corner_radius = CornerRadius::same(10);
    style.spacing.item_spacing = Vec2::new(8.0, 7.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    ctx.set_style(style);
}

fn no_resource_notice(ui: &mut egui::Ui) {
    ui.add_space(30.0);
    ui.colored_label(Color32::from_rgb(225, 110, 110), "This save has no [resource] block - it was truncated.");
    ui.label("Open Diagnostics and click Auto-repair to reconstruct it.");
}


//claude tests below

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(text: &str) -> EditorApp {
        let (doc, report) = rtv_save_core::validate::validate(text);
        EditorApp {
            project_path: "X:/RTVReversed".into(),
            catalog: Catalog::default(),
            file_path: None,
            doc: Some(doc),
            report: Some(report),
            repair_log: Vec::new(),
            view: View::Character,
            container: "inventory",
            status: String::new(),
            dirty: false,
            add_search: String::new(),
            selected: None,
            drag: None,
            equip_target: None,
            themed: true,
        }
    }

    /// Render a full Character frame headlessly to catch layout/paint panics.
    fn render_frame(app: &mut EditorApp) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1240.0, 820.0)));
        let _ = ctx.run(input, |ctx| {
            egui::SidePanel::left("l").exact_width(312.0).show(ctx, |ui| app.left_panel(ui));
            egui::SidePanel::right("r").exact_width(336.0).show(ctx, |ui| app.right_panel(ui));
            egui::CentralPanel::default().show(ctx, |ui| app.center_panel(ui));
        });
    }

    #[test]
    fn character_view_renders_without_panic() {
        let good = include_str!("../../core/tests/data/good.tres");
        let mut app = app_with(good);
        render_frame(&mut app);
        // Select an item and render again (exercises the selected panel + bars).
        app.selected = app
            .doc
            .as_ref()
            .map(|d| edit::list_slots(d, "inventory", &app.catalog))
            .and_then(|s| s.first().map(|x| ("inventory".to_string(), x.sub_id.clone())));
        render_frame(&mut app);
        // Switch to the stash container and render.
        app.container = "catalog";
        render_frame(&mut app);

        // Select an equipped item (exercises Change/Unequip buttons).
        app.container = "inventory";
        app.selected = app
            .doc
            .as_ref()
            .map(|d| edit::list_slots(d, "equipment", &app.catalog))
            .and_then(|s| s.first().map(|x| ("equipment".to_string(), x.sub_id.clone())));
        render_frame(&mut app);

        // Open the equip picker for a slot (exercises the equip card).
        app.equip_target = Some("Primary".to_string());
        render_frame(&mut app);
    }

    #[test]
    fn diagnostics_view_renders_for_corrupt_save() {
        let corrupt = include_str!("../../core/tests/data/corrupt.tres");
        let mut app = app_with(corrupt);
        app.view = View::Diagnostics;
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1240.0, 820.0)));
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.diagnostics_tab(ui));
        });
    }
}
