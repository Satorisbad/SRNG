use eframe::egui;
use srng_studio::{convert_svg, render_srng, write_png, RgbaImage, StudioDiagnostic};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 960.0])
            .with_min_inner_size([1024.0, 680.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SRNG Studio",
        options,
        Box::new(move |cc| Ok(Box::new(StudioApp::new(cc, initial.clone())))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoTab {
    Diagnostics,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Svg,
    Srng,
}

struct StudioApp {
    path: Option<PathBuf>,
    source_name: String,
    source_kind: Option<SourceKind>,
    svg_source: String,
    srng_source: String,
    diagnostics: Vec<StudioDiagnostic>,
    original_image: Option<RgbaImage>,
    rendered_image: Option<RgbaImage>,
    original_texture: Option<egui::TextureHandle>,
    rendered_texture: Option<egui::TextureHandle>,
    status: String,
    info_tab: InfoTab,
    preview_zoom: f32,
    watch_file: bool,
    last_modified: Option<SystemTime>,
    last_watch_check: Instant,
}

impl StudioApp {
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            path: None,
            source_name: "untitled.srng".to_string(),
            source_kind: None,
            svg_source: String::new(),
            srng_source: String::new(),
            diagnostics: Vec::new(),
            original_image: None,
            rendered_image: None,
            original_texture: None,
            rendered_texture: None,
            status: "Drop an SRNG or SVG file anywhere in the window, or choose Open.".to_string(),
            info_tab: InfoTab::Diagnostics,
            preview_zoom: 1.0,
            watch_file: false,
            last_modified: None,
            last_watch_check: Instant::now(),
        };
        if let Some(path) = initial {
            app.load_path(&path, &cc.egui_ctx);
        }
        app
    }

    fn load_path(&mut self, path: &Path, ctx: &egui::Context) {
        let kind = match source_kind_from_path(path) {
            Some(kind) => kind,
            None => {
                self.status = format!("Unsupported file type: {}", path.display());
                return;
            }
        };

        match fs::read_to_string(path) {
            Ok(source) => {
                let name = path
                    .file_name()
                    .and_then(|p| p.to_str())
                    .unwrap_or(match kind {
                        SourceKind::Svg => "untitled.svg",
                        SourceKind::Srng => "untitled.srng",
                    })
                    .to_string();
                match kind {
                    SourceKind::Svg => self.load_svg_text(source, name, Some(path.to_path_buf()), ctx),
                    SourceKind::Srng => self.load_srng_text(source, name, Some(path.to_path_buf()), ctx),
                }
                self.last_modified = modified_time(path);
                self.preview_zoom = 1.0;
            }
            Err(error) => {
                self.status = format!("Could not open {}: {error}", path.display());
            }
        }
    }

    fn load_svg_text(
        &mut self,
        svg: String,
        name: String,
        path: Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.path = path;
        self.source_kind = Some(SourceKind::Svg);
        self.source_name = if name.trim().is_empty() {
            "dropped.svg".to_string()
        } else {
            name
        };
        self.svg_source = svg;
        self.convert(ctx);
    }

    fn load_srng_text(
        &mut self,
        srng: String,
        name: String,
        path: Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.path = path;
        self.source_kind = Some(SourceKind::Srng);
        self.source_name = if name.trim().is_empty() {
            "dropped.srng".to_string()
        } else {
            name
        };
        self.svg_source.clear();
        self.original_image = None;
        self.original_texture = None;
        self.srng_source = srng;
        self.render_edited_srng(ctx);
        if self.rendered_image.is_some() {
            self.status = format!("Opened and rendered {}", self.source_name);
        }
    }

    fn convert(&mut self, ctx: &egui::Context) {
        let outcome = convert_svg(&self.svg_source, &self.source_name);
        self.srng_source = outcome.srng;
        self.diagnostics = outcome.diagnostics;
        self.original_image = outcome.original;
        self.rendered_image = outcome.rendered;
        self.refresh_textures(ctx);
        self.status = if self.diagnostics.iter().any(|d| d.severity == "error") {
            "Conversion completed with errors. See Diagnostics.".to_string()
        } else if self.diagnostics.iter().any(|d| d.severity == "warning") {
            "Conversion completed with warnings. See Diagnostics.".to_string()
        } else {
            "Conversion and render completed successfully.".to_string()
        };
    }

    fn render_edited_srng(&mut self, ctx: &egui::Context) {
        let source_name = if self.source_name.ends_with(".srng") {
            self.source_name.as_str()
        } else {
            "studio.srng"
        };
        let (image, diagnostics) = render_srng(&self.srng_source, source_name);
        self.rendered_image = image;
        self.diagnostics = diagnostics;
        self.refresh_rendered_texture(ctx);
        self.status = if self.rendered_image.is_some() {
            if self.diagnostics.iter().any(|d| d.severity == "warning") {
                "Rendered with warnings. See Diagnostics.".to_string()
            } else {
                "Rendered the current SRNG text.".to_string()
            }
        } else {
            "SRNG render failed. See Diagnostics.".to_string()
        };
    }

    fn refresh_textures(&mut self, ctx: &egui::Context) {
        self.original_texture = self
            .original_image
            .as_ref()
            .map(|image| load_texture(ctx, "svg-original", image));
        self.refresh_rendered_texture(ctx);
    }

    fn refresh_rendered_texture(&mut self, ctx: &egui::Context) {
        self.rendered_texture = self
            .rendered_image
            .as_ref()
            .map(|image| load_texture(ctx, "srng-rendered", image));
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SRNG and SVG", &["srng", "svg"])
            .add_filter("SRNG", &["srng"])
            .add_filter("SVG", &["svg"])
            .pick_file()
        {
            self.load_path(&path, ctx);
        }
    }

    fn save_srng(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("SRNG", &["srng"]);
        if let Some(stem) = Path::new(&self.source_name)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            dialog = dialog.set_file_name(format!("{stem}.srng"));
        }
        if let Some(path) = dialog.save_file() {
            match fs::write(&path, &self.srng_source) {
                Ok(()) => {
                    self.status = format!("Saved {}", path.display());
                    if self.source_kind == Some(SourceKind::Srng) {
                        self.path = Some(path.clone());
                        self.source_name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("untitled.srng")
                            .to_string();
                        self.last_modified = modified_time(&path);
                    }
                }
                Err(error) => self.status = format!("Could not save {}: {error}", path.display()),
            }
        }
    }

    fn save_png(&mut self) {
        let Some(image) = self.rendered_image.as_ref() else {
            self.status = "Nothing has been rendered yet.".to_string();
            return;
        };
        let mut dialog = rfd::FileDialog::new().add_filter("PNG", &["png"]);
        if let Some(stem) = Path::new(&self.source_name)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            dialog = dialog.set_file_name(format!("{stem}-srng.png"));
        }
        if let Some(path) = dialog.save_file() {
            match write_png(&path, image) {
                Ok(()) => self.status = format!("Saved {}", path.display()),
                Err(error) => self.status = format!("Could not save {}: {error}", path.display()),
            }
        }
    }

    fn reload_current(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.path.clone() {
            self.load_path(&path, ctx);
        } else {
            self.status = "Current source has no file path to reload.".to_string();
        }
    }

    fn check_file_watch(&mut self, ctx: &egui::Context) {
        if !self.watch_file || self.last_watch_check.elapsed() < Duration::from_millis(750) {
            return;
        }
        self.last_watch_check = Instant::now();

        let Some(path) = self.path.clone() else {
            return;
        };
        let modified = modified_time(&path);
        if modified.is_some() && modified != self.last_modified {
            self.load_path(&path, ctx);
            if self.source_kind.is_some() {
                self.status = format!("Auto-reloaded {} after a disk change.", self.source_name);
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path.as_ref() {
                self.load_path(path, ctx);
                return;
            }

            if let Some(bytes) = file.bytes.as_ref() {
                match String::from_utf8(bytes.to_vec()) {
                    Ok(source) => {
                        let name = if file.name.trim().is_empty() {
                            "dropped.srng".to_string()
                        } else {
                            file.name.clone()
                        };
                        match source_kind_from_name(&name) {
                            Some(SourceKind::Svg) => self.load_svg_text(source, name, None, ctx),
                            Some(SourceKind::Srng) => self.load_srng_text(source, name, None, ctx),
                            None => self.status = "Dropped text is not a recognized .srng or .svg file.".to_string(),
                        }
                        return;
                    }
                    Err(_) => {
                        self.status = "Dropped file was not valid UTF-8 text.".to_string();
                    }
                }
            }
        }
    }

    fn diagnostics_text(&self) -> String {
        if self.diagnostics.is_empty() {
            return "No diagnostics.".to_string();
        }
        self.diagnostics
            .iter()
            .map(|d| format!("{}[{}] {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn metadata_text(&self) -> String {
        let metadata = self
            .srng_source
            .lines()
            .filter(|line| line.trim_start().starts_with("svg-"))
            .map(str::trim)
            .collect::<Vec<_>>();
        if metadata.is_empty() {
            "No preserved SVG metadata.".to_string()
        } else {
            metadata.join("\n")
        }
    }

    fn copy_text(&mut self, ctx: &egui::Context, label: &str, text: String) {
        ctx.copy_text(text);
        self.status = format!("Copied {label} to clipboard.");
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_dropped_files(ctx);
        self.check_file_watch(ctx);
        let dragging_files = ctx.input(|input| !input.raw.hovered_files.is_empty());

        egui::TopBottomPanel::top("toolbar")
            .exact_height(58.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("SRNG Studio").strong().size(20.0));
                    ui.separator();
                    if ui.button("Open").on_hover_text("Choose an SRNG or SVG file").clicked() {
                        self.open_dialog(ctx);
                    }
                    if ui
                        .add_enabled(self.path.is_some(), egui::Button::new("Reload"))
                        .on_hover_text("Reload the current file from disk")
                        .clicked()
                    {
                        self.reload_current(ctx);
                    }
                    ui.add_enabled_ui(self.path.is_some(), |ui| {
                        ui.checkbox(&mut self.watch_file, "Watch")
                            .on_hover_text("Automatically reload when the file changes on disk");
                    });
                    if ui
                        .add_enabled(!self.svg_source.is_empty(), egui::Button::new("Convert SVG"))
                        .on_hover_text("Convert the current SVG source to SRNG")
                        .clicked()
                    {
                        self.convert(ctx);
                    }
                    if ui
                        .add_enabled(!self.srng_source.is_empty(), egui::Button::new("Render SRNG"))
                        .on_hover_text("Render the current editable SRNG text")
                        .clicked()
                    {
                        self.render_edited_srng(ctx);
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!self.srng_source.is_empty(), egui::Button::new("Save SRNG"))
                        .clicked()
                    {
                        self.save_srng();
                    }
                    if ui
                        .add_enabled(self.rendered_image.is_some(), egui::Button::new("Save PNG"))
                        .clicked()
                    {
                        self.save_png();
                    }
                    ui.separator();
                    zoom_controls(ui, &mut self.preview_zoom);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let name = if self.source_kind.is_none() {
                            "No file loaded"
                        } else {
                            &self.source_name
                        };
                        ui.label(egui::RichText::new(name).monospace());
                    });
                });
            });

        egui::TopBottomPanel::bottom("status")
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.small(&self.status);
                });
            });

        egui::TopBottomPanel::bottom("details")
            .resizable(true)
            .default_height(190.0)
            .min_height(120.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.info_tab, InfoTab::Diagnostics, "Diagnostics");
                    ui.selectable_value(&mut self.info_tab, InfoTab::Metadata, "SVG metadata");
                    ui.separator();
                    match self.info_tab {
                        InfoTab::Diagnostics => {
                            if ui.button("Copy diagnostics").clicked() {
                                self.copy_text(ctx, "diagnostics", self.diagnostics_text());
                            }
                        }
                        InfoTab::Metadata => {
                            if ui.button("Copy metadata").clicked() {
                                self.copy_text(ctx, "metadata", self.metadata_text());
                            }
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.info_tab {
                        InfoTab::Diagnostics => {
                            if self.diagnostics.is_empty() {
                                ui.label("No diagnostics.");
                            } else {
                                for diagnostic in &self.diagnostics {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}[{}]",
                                                diagnostic.severity, diagnostic.code
                                            ))
                                            .monospace()
                                            .strong(),
                                        );
                                        ui.label(&diagnostic.message);
                                    });
                                }
                            }
                        }
                        InfoTab::Metadata => {
                            ui.monospace(self.metadata_text());
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            if self.source_kind.is_none() {
                let available = ui.available_size();
                ui.allocate_ui_with_layout(
                    available,
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space((available.y * 0.28).max(30.0));
                        ui.label(egui::RichText::new("Drop an SRNG or SVG file here").strong().size(26.0));
                        ui.add_space(8.0);
                        ui.label("Drag from your file manager, or use Open.");
                        ui.add_space(16.0);
                        if ui.button("Open").clicked() {
                            self.open_dialog(ctx);
                        }
                    },
                );
                return;
            }

            match self.source_kind {
                Some(SourceKind::Svg) => {
                    let preview_height = (ui.available_height() * 0.50).max(260.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), preview_height),
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            let half = (ui.available_width() - 12.0) / 2.0;
                            ui.allocate_ui_with_layout(
                                egui::vec2(half, preview_height),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    panel_header(ui, "Original SVG", "Source rendered by resvg");
                                    preview_panel(
                                        ui,
                                        self.original_texture.as_ref(),
                                        self.original_image.as_ref(),
                                        "SVG preview unavailable.",
                                        self.preview_zoom,
                                    );
                                },
                            );
                            ui.separator();
                            ui.allocate_ui_with_layout(
                                egui::vec2(half, preview_height),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    panel_header(ui, "SRNG Render", "Rendered by the SRNG CPU renderer");
                                    preview_panel(
                                        ui,
                                        self.rendered_texture.as_ref(),
                                        self.rendered_image.as_ref(),
                                        "SRNG preview unavailable.",
                                        self.preview_zoom,
                                    );
                                },
                            );
                        },
                    );
                }
                Some(SourceKind::Srng) => {
                    let preview_height = (ui.available_height() * 0.58).max(300.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), preview_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            panel_header(ui, "SRNG Render", "Native SRNG document preview; scroll to pan");
                            preview_panel(
                                ui,
                                self.rendered_texture.as_ref(),
                                self.rendered_image.as_ref(),
                                "SRNG preview unavailable.",
                                self.preview_zoom,
                            );
                        },
                    );
                }
                None => {}
            }

            ui.separator();
            ui.horizontal(|ui| {
                panel_header(
                    ui,
                    if self.source_kind == Some(SourceKind::Svg) { "Generated SRNG" } else { "SRNG Source" },
                    "Editable before re-rendering",
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Copy SRNG").clicked() {
                        self.copy_text(ctx, "SRNG", self.srng_source.clone());
                    }
                    if self.source_kind == Some(SourceKind::Svg) && ui.button("Copy SVG source").clicked() {
                        self.copy_text(ctx, "SVG source", self.svg_source.clone());
                    }
                });
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.srng_source)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(16),
            );
        });

        if dragging_files {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop-overlay"),
            ));
            let rect = ctx.screen_rect();
            painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(180));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop SRNG or SVG to open",
                egui::FontId::proportional(32.0),
                egui::Color32::WHITE,
            );
        }
    }
}

fn source_kind_from_path(path: &Path) -> Option<SourceKind> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(source_kind_from_extension)
}

fn source_kind_from_name(name: &str) -> Option<SourceKind> {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(source_kind_from_extension)
}

fn source_kind_from_extension(ext: &str) -> Option<SourceKind> {
    if ext.eq_ignore_ascii_case("svg") {
        Some(SourceKind::Svg)
    } else if ext.eq_ignore_ascii_case("srng") {
        Some(SourceKind::Srng)
    } else {
        None
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn zoom_controls(ui: &mut egui::Ui, zoom: &mut f32) {
    if ui.small_button("-").on_hover_text("Zoom out").clicked() {
        *zoom = (*zoom / 1.25).clamp(0.1, 8.0);
    }
    ui.label(format!("{:.0}%", *zoom * 100.0));
    if ui.small_button("+").on_hover_text("Zoom in").clicked() {
        *zoom = (*zoom * 1.25).clamp(0.1, 8.0);
    }
    if ui.small_button("Fit").on_hover_text("Reset preview zoom").clicked() {
        *zoom = 1.0;
    }
}

fn panel_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(title).strong().size(17.0));
        ui.label(egui::RichText::new(subtitle).weak());
    });
    ui.add_space(4.0);
}

fn preview_panel(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    image: Option<&RgbaImage>,
    unavailable: &str,
    zoom: f32,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let available = ui.available_size();
        if let (Some(texture), Some(image)) = (texture, image) {
            let image_size = egui::vec2(image.width as f32, image.height as f32);
            let fit = (available.x / image_size.x)
                .min(available.y / image_size.y)
                .min(1.0)
                .max(0.01);
            let size = image_size * (fit * zoom).clamp(0.01, 8.0);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let remaining = ui.available_size();
                    let pad_x = ((remaining.x - size.x) * 0.5).max(0.0);
                    let pad_y = ((remaining.y - size.y) * 0.5).max(0.0);
                    ui.add_space(pad_y);
                    ui.horizontal(|ui| {
                        ui.add_space(pad_x);
                        ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                    });
                });
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(unavailable);
            });
        }
    });
}

fn load_texture(ctx: &egui::Context, name: &str, image: &RgbaImage) -> egui::TextureHandle {
    let color = egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.pixels);
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}
