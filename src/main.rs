mod model;
mod render;
mod vox;
use eframe::egui::{self, Color32, RichText, Vec2};
use model::{Mesh, Volume};
use render::{Camera, Face};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
const PURPLE: Color32 = Color32::from_rgb(121, 81, 203);
const INK: Color32 = Color32::from_rgb(42, 39, 54);
const MUTED: Color32 = Color32::from_rgb(132, 126, 145);
const BG: Color32 = Color32::from_rgb(246, 245, 249);
#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Orbit,
    Add,
    Remove,
    Paint,
    Pick,
}
enum Job {
    Imported(Result<(Mesh, String), String>),
    Generated(Volume),
    Opened(Result<(Volume, String), String>),
    Saved(Result<String, String>),
}
struct Studio {
    volume: Volume,
    faces: Vec<Face>,
    mesh: Option<Arc<Mesh>>,
    source_faces: Vec<Face>,
    camera: Camera,
    name: String,
    source_name: String,
    resolution: u32,
    solid: bool,
    textures: bool,
    grid: bool,
    outlines: bool,
    show_source: bool,
    tool: Tool,
    color: [u8; 3],
    history: Vec<Volume>,
    redo: Vec<Volume>,
    dirty: bool,
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Job>,
    busy: bool,
    status: String,
    error: bool,
    pending: Option<Instant>,
    help: bool,
    confirm: Option<Intent>,
    after_save: Option<Intent>,
    started: Instant,
}
#[derive(Clone)]
enum Intent {
    New,
    Import,
    Open,
    Demo(u8),
    Quit,
}
impl Studio {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = Color32::WHITE;
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.override_text_color = Some(INK);
        style.visuals.selection.bg_fill = PURPLE;
        style.visuals.selection.stroke = egui::Stroke::new(1.0f32, Color32::WHITE);
        style.visuals.widgets.inactive.bg_fill = BG;
        style.visuals.widgets.inactive.weak_bg_fill = BG;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(234, 227, 248);
        style.spacing.item_spacing = Vec2::new(8., 10.);
        style.spacing.button_padding = Vec2::new(12., 9.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
        cc.egui_ctx.set_style(style);
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "symbols".into(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/fonts/DejaVuSans.ttf"
            ))),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .push("symbols".into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .push("symbols".into());
        cc.egui_ctx.set_fonts(fonts);
        let volume = demo(0);
        let faces = render::volume_faces(&volume);
        let (tx, rx) = mpsc::channel();
        let mut studio = Self {
            volume,
            faces,
            mesh: None,
            source_faces: vec![],
            camera: Camera::default(),
            name: "Woodland mushroom".into(),
            source_name: String::new(),
            resolution: 32,
            solid: true,
            textures: true,
            grid: true,
            outlines: false,
            show_source: false,
            tool: Tool::Orbit,
            color: [183, 111, 193],
            history: vec![],
            redo: vec![],
            dirty: false,
            tx,
            rx,
            busy: false,
            status: "Bereit. Erstelle etwas Besonderes.".into(),
            error: false,
            pending: None,
            help: false,
            confirm: None,
            after_save: None,
            started: Instant::now(),
        };
        let args: Vec<_> = std::env::args().collect();
        if args.get(1).is_some_and(|a| a == "--open") {
            if let Some(path) = args.get(2) {
                studio.open_path(PathBuf::from(path), &cc.egui_ctx);
            }
        }
        studio
    }
    fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.busy = true;
        self.message("Datei wird geöffnet …", false);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let job = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("vox"))
            {
                Job::Opened(vox::load(&path).map(|v| (v, name)))
            } else {
                Job::Imported(Mesh::load(&path).map(|m| (m, name)))
            };
            let _ = tx.send(job);
            ctx.request_repaint();
        });
    }
    fn snapshot(&mut self) {
        self.history.push(self.volume.clone());
        while self.history.len() > 1
            && (self.history.len() > 30
                || self.history.iter().map(|v| v.cells.len()).sum::<usize>() > 2_000_000)
        {
            self.history.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }
    fn rebuild(&mut self) {
        self.faces = render::volume_faces(&self.volume);
    }
    fn undo(&mut self) {
        if let Some(v) = self.history.pop() {
            self.redo.push(std::mem::replace(&mut self.volume, v));
            self.rebuild();
            self.dirty = true;
        }
    }
    fn redo(&mut self) {
        if let Some(v) = self.redo.pop() {
            self.history.push(std::mem::replace(&mut self.volume, v));
            self.rebuild();
            self.dirty = true;
        }
    }
    fn message(&mut self, s: impl Into<String>, error: bool) {
        self.status = s.into();
        self.error = error;
    }
    fn generate(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        if let Some(mesh) = self.mesh.clone() {
            self.busy = true;
            self.pending = None;
            self.message("Modell wird voxelisiert …", false);
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            let n = self.resolution;
            let solid = self.solid;
            let textures = self.textures;
            std::thread::spawn(move || {
                let v = mesh.voxelize(n, solid, textures);
                let _ = tx.send(Job::Generated(v));
                ctx.request_repaint();
            });
        }
    }
    fn request(&mut self, intent: Intent, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        if self.dirty {
            self.confirm = Some(intent);
        } else {
            self.perform(intent, ctx);
        }
    }
    fn perform(&mut self, intent: Intent, ctx: &egui::Context) {
        match intent {
            Intent::Quit => {
                self.dirty = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Intent::New | Intent::Demo(_) => {
                let id = if let Intent::Demo(i) = intent {
                    Some(i)
                } else {
                    None
                };
                self.volume = id.map(demo).unwrap_or(Volume {
                    size: 32,
                    cells: HashMap::from([([16, 0, 16], self.color)]),
                });
                self.name = match id {
                    Some(0) => "Woodland mushroom",
                    Some(1) => "Desert arch",
                    Some(_) => "Little island",
                    None => "Neues Asset",
                }
                .into();
                self.mesh = None;
                self.source_faces.clear();
                self.source_name.clear();
                self.history.clear();
                self.redo.clear();
                self.show_source = false;
                self.dirty = false;
                self.resolution = 32;
                self.pending = None;
                self.camera = Camera::default();
                self.rebuild();
                self.message("Neues Projekt bereit.", false);
            }
            Intent::Import | Intent::Open => {
                self.pending = None;
                let import = matches!(intent, Intent::Import);
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                self.busy = true;
                self.message(
                    if import {
                        "Modell auswählen …"
                    } else {
                        "VOX-Datei auswählen …"
                    },
                    false,
                );
                std::thread::spawn(move || {
                    let dialog = rfd::FileDialog::new();
                    let file = if import {
                        dialog.add_filter("3D-Modelle", &["obj", "blend"])
                    } else {
                        dialog.add_filter("MagicaVoxel", &["vox"])
                    }
                    .pick_file();
                    let job = if let Some(p) = file {
                        let name = p
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        if import {
                            Job::Imported(Mesh::load(&p).map(|m| (m, name)))
                        } else {
                            Job::Opened(vox::load(&p).map(|v| (v, name)))
                        }
                    } else {
                        Job::Saved(Ok("Auswahl abgebrochen.".into()))
                    };
                    let _ = tx.send(job);
                    ctx.request_repaint();
                });
            }
        }
    }
    fn save(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let volume = self.volume.clone();
        let name = format!("{}.vox", self.name.trim_end_matches(".vox"));
        self.busy = true;
        std::thread::spawn(move || {
            let r = if let Some(p) = rfd::FileDialog::new()
                .add_filter("MagicaVoxel", &["vox"])
                .set_file_name(&name)
                .save_file()
            {
                let p = p.with_extension("vox");
                vox::save(&volume, &p).map(|_| format!("Gespeichert: {}", p.display()))
            } else {
                Ok("Export abgebrochen.".into())
            };
            let _ = tx.send(Job::Saved(r));
            ctx.request_repaint();
        });
    }
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(job) = self.rx.try_recv() {
            self.busy = false;
            match job {
                Job::Imported(Ok((m, name))) => {
                    let warnings = m.warnings.join(" · ");
                    self.source_faces = render::mesh_faces(&m);
                    self.mesh = Some(Arc::new(m));
                    self.source_name = name.clone();
                    self.name = PathBuf::from(name)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into();
                    self.show_source = true;
                    self.history.clear();
                    self.redo.clear();
                    self.dirty = false;
                    self.camera = Camera::default();
                    self.generate(ctx);
                    if !warnings.is_empty() {
                        self.message(warnings, true);
                    }
                }
                Job::Imported(Err(e)) | Job::Opened(Err(e)) => self.message(e, true),
                Job::Generated(v) => {
                    self.snapshot();
                    self.volume = v;
                    self.rebuild();
                    self.show_source = false;
                    if !self.error {
                        self.message(
                            format!(
                                "Voxelisierung abgeschlossen · {} Voxel",
                                self.volume.cells.len()
                            ),
                            false,
                        );
                    }
                }
                Job::Opened(Ok((v, name))) => {
                    self.volume = v;
                    self.resolution = self.volume.size.min(128);
                    self.name = name.trim_end_matches(".vox").into();
                    self.mesh = None;
                    self.source_faces.clear();
                    self.source_name.clear();
                    self.show_source = false;
                    self.history.clear();
                    self.redo.clear();
                    self.dirty = false;
                    self.rebuild();
                    self.message("VOX-Modell geöffnet.", false);
                }
                Job::Saved(Ok(s)) => {
                    if s.starts_with("Gespeichert:") {
                        self.dirty = false;
                    }
                    let saved = s.starts_with("Gespeichert:");
                    self.message(s, false);
                    if let Some(intent) = self.after_save.take() {
                        if saved {
                            self.perform(intent, ctx);
                        }
                    }
                }
                Job::Saved(Err(e)) => {
                    self.after_save = None;
                    self.message(e, true);
                }
            }
        }
    }
    fn left(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(20.);
        section(ui, "WORKSPACE");
        ui.add_space(6.);
        if wide_button(ui, "+   Neues Asset", false) {
            self.request(Intent::New, ctx);
        }
        if wide_button(ui, "↥   Modell importieren", true) {
            self.request(Intent::Import, ctx);
        }
        if wide_button(ui, "▧   .vox öffnen", false) {
            self.request(Intent::Open, ctx);
        }
        ui.add_space(22.);
        section(ui, "SZENE");
        ui.add_space(8.);
        egui::Frame::new()
            .fill(Color32::from_rgb(241, 235, 250))
            .corner_radius(7.)
            .inner_margin(12.)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.colored_label(PURPLE, "◆");
                    ui.label(RichText::new(&self.name).strong().size(13.));
                });
                ui.label(
                    RichText::new(format!(
                        "     {} Voxel · {}³ Raster",
                        self.volume.cells.len(),
                        self.volume.size
                    ))
                    .size(11.)
                    .color(MUTED),
                );
            });
        ui.add_space(24.);
        section(ui, "STARTPUNKTE");
        ui.add_space(8.);
        for (i, title, subtitle, color) in [
            (0, "Woodland mushroom", "Organisch · 32³", [176, 111, 189]),
            (1, "Desert arch", "Architektur · 32³", [212, 158, 108]),
            (2, "Little island", "Landschaft · 32³", [114, 161, 132]),
        ] {
            let r = egui::Frame::new()
                .fill(BG)
                .corner_radius(8.)
                .inner_margin(10.)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(30.), egui::Sense::hover());
                        ui.painter().rect_filled(
                            rect.shrink(3.),
                            5.,
                            Color32::from_rgb(color[0], color[1], color[2]),
                        );
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).size(12.).strong());
                            ui.label(RichText::new(subtitle).size(10.).color(MUTED));
                        });
                    });
                })
                .response;
            if ui
                .interact(r.rect, ui.id().with(i), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.request(Intent::Demo(i), ctx);
            }
        }
        ui.add_space(20.);
        ui.separator();
        ui.add_space(12.);
        ui.label(
            RichText::new("Kleine Würfel.\nGroße Möglichkeiten.")
                .size(18.)
                .color(MUTED),
        );
        ui.add_space(8.);
        ui.label(
            RichText::new("Dein nächstes Asset beginnt hier.")
                .size(11.)
                .color(MUTED),
        );
    }
    fn right(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(20.);
        section(ui, "ASSET-EIGENSCHAFTEN");
        ui.add_space(10.);
        ui.label(RichText::new("Name").size(12.).color(MUTED));
        if ui.text_edit_singleline(&mut self.name).changed() {
            self.dirty = true;
        }
        ui.add_space(14.);
        ui.separator();
        ui.add_space(12.);
        section(ui, "MODELL → VOXEL");
        ui.add_space(8.);
        if let Some(m) = &self.mesh {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 130.), egui::Sense::hover());
            ui.painter().rect_filled(rect, 8., BG);
            render::draw(
                ui,
                rect,
                &Camera::default(),
                &self.source_faces,
                false,
                false,
                None,
            );
            ui.label(RichText::new(&self.source_name).size(12.).strong());
            ui.label(
                RichText::new(format!(
                    "{} Dreiecke · {} Texturen",
                    m.triangles.len(),
                    m.materials.iter().filter(|m| m.texture.is_some()).count()
                ))
                .size(11.)
                .color(MUTED),
            );
        } else {
            egui::Frame::new()
                .fill(BG)
                .corner_radius(8.)
                .inner_margin(16.)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("↥  Von Mesh zu Magie").strong().color(PURPLE));
                    ui.label(
                        RichText::new(
                            "Importiere ein 3D-Modell und verwandle es in ein Voxel-Asset.",
                        )
                        .size(12.)
                        .color(MUTED),
                    );
                    ui.label(
                        RichText::new(".OBJ + .MTL    /    .BLEND")
                            .monospace()
                            .size(10.)
                            .color(MUTED),
                    );
                });
        }
        ui.add_space(12.);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Auflösung").strong().size(12.));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(PURPLE, format!("{}³", self.resolution));
            });
        });
        let mut changed = false;
        ui.add_enabled_ui(self.mesh.is_some() && !self.busy, |ui| {
            changed |= ui
                .add(egui::Slider::new(&mut self.resolution, 4..=128).show_value(false))
                .changed();
            ui.horizontal(|ui| {
                ui.label(RichText::new("4 · Grob").size(10.).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("128 · Fein").size(10.).color(MUTED));
                });
            });
            changed |= ui
                .checkbox(&mut self.textures, "Texturen übernehmen")
                .on_hover_text("UV-basierte Texturfarben und Materialfarben verwenden")
                .changed();
            changed |= ui
                .checkbox(&mut self.solid, "Geschlossene Hohlräume füllen")
                .changed();
            ui.add_space(4.);
            if wide_button(ui, "◆   Voxelisieren", true) {
                self.generate(ctx);
            }
        });
        if changed {
            self.pending = Some(Instant::now());
        }
        if self.mesh.is_some() {
            ui.label(
                RichText::new("Vorschau aktualisiert sich automatisch.")
                    .size(10.)
                    .color(MUTED),
            );
        }
        ui.add_space(16.);
        ui.separator();
        ui.add_space(12.);
        section(ui, "FARBPALETTE");
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(&mut self.color);
            ui.label(
                RichText::new(format!(
                    "#{:02X}{:02X}{:02X}",
                    self.color[0], self.color[1], self.color[2]
                ))
                .monospace()
                .size(11.)
                .color(MUTED),
            );
        });
        let colors = [
            [183, 111, 193],
            [221, 165, 210],
            [239, 210, 221],
            [244, 229, 206],
            [172, 137, 101],
            [109, 156, 127],
            [90, 116, 135],
            [63, 63, 80],
            [225, 147, 105],
            [255, 211, 127],
            [161, 195, 183],
            [213, 220, 229],
        ];
        egui::Grid::new("palette")
            .spacing(Vec2::splat(6.))
            .show(ui, |ui| {
                for (i, c) in colors.iter().enumerate() {
                    let (r, response) =
                        ui.allocate_exact_size(Vec2::splat(27.), egui::Sense::click());
                    ui.painter()
                        .rect_filled(r.shrink(2.), 5., Color32::from_rgb(c[0], c[1], c[2]));
                    if self.color == *c {
                        ui.painter().rect_stroke(
                            r,
                            6.,
                            egui::Stroke::new(1.5f32, PURPLE),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.clicked() {
                        self.color = *c;
                        self.tool = Tool::Paint;
                    }
                    if i % 6 == 5 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(16.);
        ui.separator();
        ui.add_space(10.);
        section(ui, "ANSICHT");
        ui.checkbox(&mut self.grid, "Bodenraster");
        ui.checkbox(&mut self.outlines, "Voxel-Kanten");
        ui.add_enabled_ui(self.mesh.is_some(), |ui| {
            ui.checkbox(&mut self.show_source, "Originalmodell anzeigen");
        });
    }
    fn viewport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("3D WORKSPACE")
                    .size(10.)
                    .strong()
                    .color(MUTED),
            );
            ui.add_space(14.);
            if ui.selectable_label(!self.show_source, "Voxel").clicked() {
                self.show_source = false;
            }
            if ui
                .add_enabled(
                    self.mesh.is_some(),
                    egui::Button::new("Original").selected(self.show_source),
                )
                .clicked()
            {
                self.show_source = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⊙  Ansicht zurücksetzen").clicked() {
                    self.camera = Camera::default();
                }
                ui.label(RichText::new("Orthografisch").size(11.).color(MUTED));
            });
        });
        ui.add_space(8.);
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(available.x, (available.y - 46.).max(100.)),
            egui::Sense::click_and_drag(),
        );
        ui.painter()
            .rect_filled(rect, 12., Color32::from_rgb(239, 237, 244));
        let input = ui.input(|i| {
            (
                i.pointer.delta(),
                i.smooth_scroll_delta.y,
                i.modifiers.shift,
            )
        });
        let orbit = response.dragged_by(egui::PointerButton::Secondary)
            || (self.tool == Tool::Orbit && response.dragged_by(egui::PointerButton::Primary));
        if orbit {
            if input.2 {
                self.camera.pan += input.0;
            } else {
                self.camera.yaw += input.0.x * 0.008;
                self.camera.pitch = (self.camera.pitch + input.0.y * 0.008).clamp(-1.3, 1.5);
            }
        }
        if response.hovered() {
            self.camera.zoom = (self.camera.zoom * (input.1 * 0.0015).exp()).clamp(0.25, 6.);
        }
        let pointer = if !self.busy && !self.show_source && self.tool != Tool::Orbit {
            response.hover_pos()
        } else {
            None
        };
        let faces = if self.show_source {
            &self.source_faces
        } else {
            &self.faces
        };
        let hit = render::draw(
            ui,
            rect,
            &self.camera,
            faces,
            self.grid,
            self.outlines,
            pointer,
        );
        let painter = ui.painter_at(rect);
        painter.text(
            rect.left_top() + Vec2::new(20., 20.),
            egui::Align2::LEFT_TOP,
            if self.show_source {
                "ORIGINAL MESH"
            } else {
                "VOXEL PREVIEW"
            },
            egui::FontId::monospace(10.),
            MUTED,
        );
        painter.text(
            rect.left_bottom() + Vec2::new(20., -20.),
            egui::Align2::LEFT_BOTTOM,
            format!(
                "{} × {} × {}",
                self.volume.size, self.volume.size, self.volume.size
            ),
            egui::FontId::monospace(11.),
            MUTED,
        );
        if self.busy {
            let r = egui::Rect::from_center_size(rect.center(), Vec2::new(240., 65.));
            painter.rect_filled(r, 12., Color32::from_rgba_unmultiplied(255, 255, 255, 242));
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "Verarbeite Modell …",
                egui::FontId::proportional(16.),
                PURPLE,
            );
        }
        if response.clicked() && !self.busy {
            if let Some(hit) = hit {
                match self.tool {
                    Tool::Pick => {
                        self.color = self.volume.cells[&hit.voxel];
                        self.tool = Tool::Paint;
                    }
                    Tool::Add => {
                        let p = std::array::from_fn(|i| hit.voxel[i] + hit.normal[i]);
                        if p.iter().all(|&v| v >= 0 && v < self.volume.size as i32) {
                            self.snapshot();
                            self.volume.cells.insert(p, self.color);
                            self.rebuild();
                        } else {
                            self.message(
                                "Rastergrenze erreicht. Wähle eine Fläche innerhalb des Rasters.",
                                true,
                            );
                        }
                    }
                    Tool::Remove => {
                        self.snapshot();
                        self.volume.cells.remove(&hit.voxel);
                        self.rebuild();
                    }
                    Tool::Paint => {
                        self.snapshot();
                        self.volume.cells.insert(hit.voxel, self.color);
                        self.rebuild();
                    }
                    Tool::Orbit => {}
                }
            } else if self.tool == Tool::Add && self.volume.cells.is_empty() && !self.show_source {
                self.snapshot();
                self.volume.cells.insert(
                    [self.volume.size as i32 / 2, 0, self.volume.size as i32 / 2],
                    self.color,
                );
                self.rebuild();
            }
        }
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    "Ziehen: Drehen    ·    Scrollen: Zoom    ·    Shift + Ziehen: Verschieben",
                )
                .size(11.)
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("Y ↑").monospace().size(11.).color(MUTED));
            });
        });
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        if self
            .pending
            .is_some_and(|t| t.elapsed() > Duration::from_millis(450))
            && !self.busy
        {
            self.generate(ctx);
        }
        if self.pending.is_some() || self.busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !ctx.wants_keyboard_input() && !self.busy {
            let keys = ctx.input(|i| {
                (
                    i.modifiers.command,
                    i.modifiers.shift,
                    i.key_pressed(egui::Key::Z),
                    i.key_pressed(egui::Key::S),
                    i.key_pressed(egui::Key::O),
                    i.key_pressed(egui::Key::Num1),
                    i.key_pressed(egui::Key::Num2),
                    i.key_pressed(egui::Key::Num3),
                    i.key_pressed(egui::Key::Num4),
                    i.key_pressed(egui::Key::Num5),
                )
            });
            if keys.0 && keys.2 {
                if keys.1 {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            if keys.0 && keys.3 {
                self.save(ctx);
            }
            if keys.0 && keys.4 {
                self.request(Intent::Import, ctx);
            }
            for (pressed, tool) in [
                (keys.5, Tool::Orbit),
                (keys.6, Tool::Add),
                (keys.7, Tool::Remove),
                (keys.8, Tool::Paint),
                (keys.9, Tool::Pick),
            ] {
                if pressed {
                    self.tool = tool;
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Intent::Quit);
        }
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(22, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(32.), egui::Sense::hover());
                    let p = ui.painter();
                    let c = r.center();
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            c + Vec2::new(0., -14.),
                            c + Vec2::new(13., -7.),
                            c,
                            c + Vec2::new(-13., -7.),
                        ],
                        Color32::from_rgb(171, 135, 227),
                        egui::Stroke::NONE,
                    ));
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            c + Vec2::new(-13., -7.),
                            c,
                            c + Vec2::new(0., 14.),
                            c + Vec2::new(-13., 7.),
                        ],
                        PURPLE,
                        egui::Stroke::NONE,
                    ));
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            c,
                            c + Vec2::new(13., -7.),
                            c + Vec2::new(13., 7.),
                            c + Vec2::new(0., 14.),
                        ],
                        Color32::from_rgb(96, 64, 163),
                        egui::Stroke::NONE,
                    ));
                    ui.add_space(6.);
                    ui.label(RichText::new("voxel").size(24.).strong());
                    ui.label(RichText::new("studio").size(24.).color(MUTED));
                    ui.add_space(12.);
                    ui.label(
                        RichText::new("CREATE IN ANOTHER DIMENSION")
                            .size(9.)
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.busy && !self.volume.cells.is_empty(),
                                egui::Button::new(
                                    RichText::new("↧   .vox exportieren").color(Color32::WHITE),
                                )
                                .fill(PURPLE)
                                .corner_radius(7.),
                            )
                            .clicked()
                        {
                            self.save(ctx);
                        }
                        if ui.button("?  Hilfe").clicked() {
                            self.help = true;
                        }
                        ui.label(
                            RichText::new(if self.dirty {
                                "●  Ungespeichert"
                            } else {
                                "●  Lokal"
                            })
                            .size(11.)
                            .color(MUTED),
                        );
                    });
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(20, 9)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    } else {
                        ui.colored_label(
                            if self.error {
                                Color32::from_rgb(195, 102, 74)
                            } else {
                                Color32::from_rgb(108, 161, 127)
                            },
                            "●",
                        );
                    }
                    egui::ScrollArea::horizontal()
                        .max_width((ui.available_width() - 120.).max(50.))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.status).size(11.).color(if self.error {
                                Color32::from_rgb(177, 92, 70)
                            } else {
                                MUTED
                            }));
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("RUST  /  v0.1")
                                .monospace()
                                .size(10.)
                                .color(MUTED),
                        );
                    });
                });
            });
        egui::SidePanel::left("library")
            .exact_width(232.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(18, 0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(!self.busy, |ui| self.left(ui, ctx));
                });
            });
        egui::SidePanel::right("properties")
            .exact_width(268.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.right(ui, ctx));
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(22, 18)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(RichText::new("Deine Ideen. In Voxeln.").size(25.).strong());
                        ui.label(
                            RichText::new(
                                "Formen entdecken. Details reduzieren. Neues erschaffen.",
                            )
                            .size(12.)
                            .color(MUTED),
                        );
                    });
                });
                ui.add_space(18.);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.busy, |ui| {
                        for (tool, icon, tip) in [
                            (Tool::Orbit, "↻", "1 · Drehen"),
                            (Tool::Add, "+", "2 · Voxel hinzufügen"),
                            (Tool::Remove, "−", "3 · Voxel entfernen"),
                            (Tool::Paint, "▨", "4 · Voxel färben"),
                            (Tool::Pick, "◉", "5 · Farbe aufnehmen"),
                        ] {
                            if ui
                                .add_sized(
                                    [36., 34.],
                                    egui::Button::new(
                                        RichText::new(icon).size(20.).color(if self.tool == tool {
                                            PURPLE
                                        } else {
                                            MUTED
                                        }),
                                    )
                                    .fill(if self.tool == tool {
                                        Color32::from_rgb(232, 222, 249)
                                    } else {
                                        Color32::WHITE
                                    })
                                    .corner_radius(7.),
                                )
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.tool = tool;
                            }
                        }
                        ui.add_space(8.);
                        ui.separator();
                        if ui
                            .add_enabled(!self.history.is_empty(), egui::Button::new("↶"))
                            .on_hover_text("Strg+Z · Rückgängig")
                            .clicked()
                        {
                            self.undo();
                        }
                        if ui
                            .add_enabled(!self.redo.is_empty(), egui::Button::new("↷"))
                            .on_hover_text("Strg+Shift+Z · Wiederholen")
                            .clicked()
                        {
                            self.redo();
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(match self.tool {
                                Tool::Orbit => "DREHEN",
                                Tool::Add => "HINZUFÜGEN",
                                Tool::Remove => "ENTFERNEN",
                                Tool::Paint => "FÄRBEN",
                                Tool::Pick => "PIPETTE",
                            })
                            .size(10.)
                            .color(MUTED),
                        );
                    });
                });
                ui.add_space(18.);
                self.viewport(ui);
            });
        if let Some(intent) = self.confirm.clone() {
            egui::Window::new("Ungespeicherte Änderungen")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        "Dieses Projekt enthält Änderungen. Vor dem Wechsel als .vox exportieren?",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Exportieren").clicked() {
                            self.after_save = Some(intent.clone());
                            self.confirm = None;
                            self.save(ctx);
                        }
                        if ui.button("Verwerfen und fortfahren").clicked() {
                            self.confirm = None;
                            self.perform(intent, ctx);
                        }
                        if ui.button("Abbrechen").clicked() {
                            self.confirm = None;
                        }
                    });
                });
        }
        egui::Window::new("Willkommen im Voxel Studio").open(&mut self.help).collapsible(false).resizable(false).default_width(460.).show(ctx,|ui|{ui.label("Importiere .obj oder .blend und passe die Auflösung an. Nach einer kurzen Pause wird die Voxelvorschau automatisch aktualisiert.");ui.add_space(10.);ui.label("1  Drehen     2  Hinzufügen     3  Entfernen\n4  Färben      5  Pipette\nRechts ziehen: Drehen · Shift: Verschieben\nMausrad: Zoom · Strg+Z: Rückgängig\nStrg+S: .vox exportieren · Strg+O: Importieren");ui.add_space(10.);ui.label("OBJ: .mtl und Texturen neben dem Modell aufbewahren.\nBLEND: Blender muss installiert sein (optional BLENDER_PATH).\nÜbernommen werden Geometrie, UV-Texturen und diffuse Farben. Prozedurale Shader vorher als Bildtextur backen.");ui.add_space(10.);ui.label("VOX speichert bis zu 255 Farben. Umfangreiche Texturen werden beim Export auf eine RGB-Palette mit 216 Farben reduziert. Mehrteilige VOX-Szenen werden nicht unterstützt.");ui.label("Erneutes Voxelisieren ersetzt manuelle Änderungen. Über Rückgängig kannst du zur vorherigen Version zurückkehren.");});
        // Optional deterministic capture for CI/manual visual verification.
        if std::env::var_os("VOXEL_SCREENSHOT").is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if std::env::var_os("VOXEL_SCREENSHOT").is_some()
            && self.started.elapsed() > Duration::from_secs(2)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let path = std::env::var("VOXEL_SCREENSHOT").unwrap();
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    let _ = image::save_buffer(
                        path,
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    );
                    self.dirty = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}
fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(10.).strong().color(MUTED));
}
fn wide_button(ui: &mut egui::Ui, text: &str, accent: bool) -> bool {
    ui.add_sized(
        [ui.available_width(), 36.],
        egui::Button::new(RichText::new(text).color(if accent { Color32::WHITE } else { INK }))
            .fill(if accent { PURPLE } else { BG })
            .corner_radius(7.),
    )
    .clicked()
}
fn demo(id: u8) -> Volume {
    let mut cells = HashMap::new();
    for z in 0..32 {
        for y in 0..32 {
            for x in 0..32 {
                let dx = x as f32 - 15.5;
                let dz = z as f32 - 15.5;
                let r = (dx * dx + dz * dz).sqrt();
                let p = [x, y, z];
                match id {
                    0 => {
                        if r < 13. && y <= 2 {
                            cells.insert(
                                p,
                                if (x + z) % 5 == 0 {
                                    [132, 165, 124]
                                } else {
                                    [154, 182, 138]
                                },
                            );
                        }
                        if r < 3.8 + (y as f32 / 10.).sin() && (3..=17).contains(&y) {
                            cells.insert(
                                p,
                                if (x + z) % 4 == 0 {
                                    [220, 200, 177]
                                } else {
                                    [242, 224, 197]
                                },
                            );
                        }
                        let profile = if (17..=27).contains(&y) {
                            12.8 * (1. - ((y as f32 - 18.) / 11.).powi(2)).max(0.).sqrt()
                        } else {
                            0.
                        };
                        if r < profile {
                            let spot = ((dx - 5.).powi(2) + (dz - 3.).powi(2) < 6.)
                                || ((dx + 6.).powi(2) + (dz + 2.).powi(2) < 8.)
                                || ((dx - 2.).powi(2) + (dz + 7.).powi(2) < 4.);
                            cells.insert(
                                p,
                                if y == 17 {
                                    [209, 162, 186]
                                } else if spot {
                                    [246, 224, 218]
                                } else if (x + y + z) % 7 == 0 {
                                    [190, 125, 199]
                                } else {
                                    [172, 103, 184]
                                },
                            );
                        }
                    }
                    1 => {
                        let a = (dx * dx + (y as f32 - 9.).powi(2)).sqrt();
                        if dz.abs() < 4.
                            && y >= 2
                            && (a < 13. && a > 7. || dx.abs() > 8. && dx.abs() < 13. && y < 10)
                        {
                            cells.insert(
                                p,
                                if y % 4 == 0 {
                                    [190, 132, 93]
                                } else {
                                    [222, 171, 122]
                                },
                            );
                        }
                        if y < 2 && r < 15. {
                            cells.insert(p, [236, 210, 169]);
                        }
                    }
                    _ => {
                        let h = 5. + ((dx * 0.25).sin() + (dz * 0.3).cos()) * 2.;
                        if r < 12. - y as f32 * 0.12 && y < h as i32 {
                            cells.insert(
                                p,
                                if y >= h as i32 - 1 {
                                    [126, 170, 135]
                                } else if y > 2 {
                                    [172, 137, 104]
                                } else {
                                    [131, 143, 150]
                                },
                            );
                        }
                        if (dx - 3.).abs() < 1.5 && (dz + 2.).abs() < 1.5 && (6..17).contains(&y) {
                            cells.insert(p, [145, 113, 87]);
                        }
                        if ((dx - 3.).powi(2) + (dz + 2.).powi(2) + (y as f32 - 18.).powi(2)).sqrt()
                            < 5.5
                        {
                            cells.insert(p, [109, 153, 122]);
                        }
                    }
                }
            }
        }
    }
    Volume { size: 32, cells }
}
fn main() -> eframe::Result {
    // Headless conversion also exercises precisely the same importer and voxelizer.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--convert") {
        if args.len() < 4 {
            eprintln!("Aufruf: voxel-studio --convert input.obj|input.blend output.vox [4..128]");
            std::process::exit(2);
        }
        let result = (|| -> Result<(), String> {
            let resolution = args
                .get(4)
                .map(|n| {
                    n.parse::<u32>()
                        .map_err(|_| "Ungültige Auflösung".to_string())
                })
                .transpose()?
                .unwrap_or(32);
            if !(4..=128).contains(&resolution) {
                return Err("Auflösung muss zwischen 4 und 128 liegen.".into());
            }
            let m = Mesh::load(std::path::Path::new(&args[2]))?;
            for w in &m.warnings {
                eprintln!("Hinweis: {w}");
            }
            let v = m.voxelize(resolution, true, true);
            vox::save(&v, std::path::Path::new(&args[3]))?;
            println!("{} Voxel → {}", v.cells.len(), args[3]);
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    eframe::run_native(
        "Voxel Studio",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 920.])
                .with_min_inner_size([1100., 760.]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(Studio::new(cc)))),
    )
}
