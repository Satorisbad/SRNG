use eframe::egui;
use srng_studio::{convert_svg, render_srng, write_png, RgbaImage, StudioDiagnostic};
use std::fs;
use std::path::{Path, PathBuf};

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

struct StudioApp {
    path: Option<PathBuf>,
    source_name: String,
    svg_source: String,
    srng_source: String,
    diagnostics: Vec<StudioDiagnostic>,
    original_image: Option<RgbaImage>,
    rendered_image: Option<RgbaImage>,
    original_texture: Option<egui::TextureHandle>,
    rendered_texture: Option<egui::TextureHandle>,
    status: String,
    info_tab: InfoTab,
}

impl StudioApp {
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            path: None,
            source_name: "untitled.svg".to_string(),
            svg_source: String::new(),
            srng_source: String::new(),
            diagnostics: Vec::new(),
            original_image: None,
            rendered_image: None,
            original_texture: None,
            rendered_texture: None,
            status: "Drop an SVG anywhere in the window, or choose Open SVG.".to_string(),
            info_tab: InfoTab::Diagnostics,
        };
        if let Some(path) = initial {
            app.load_path(&path, &cc.egui_ctx);
        }
        app
    }

    fn load_path(&mut self, path: &Path, ctx: &egui::Context) {
        match fs::read_to_string(path) {
            Ok(svg) => {
                let name = path
                    .file_name()
                    .and_then(|p| p.to_str())
                    .unwrap_or("untitled.svg")
                    .to_string();
                self.load_svg_text(svg, name, Some(path.to_path_buf()), ctx);
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
        self.source_name = if name.trim().is_empty() {
            "dropped.svg".to_string()
        } else {
            name
        };
        self.svg_source = svg;
        self.convert(ctx);
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
        let (image, diagnostics) = render_srng(&self.srng_source, "studio.srng");
        self.rendered_image = image;
        self.diagnostics = diagnostics;
        self.refresh_rendered_texture(ctx);
        self.status = if self.rendered_image.is_some() {
            "Rendered the current SRNG text.".to_string()
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
                Ok(()) => self.status = format!("Saved {}", path.display()),
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

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path.as_ref() {
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
                {
                    self.load_path(path, ctx);
                    return;
                }
                self.status = format!("Dropped file is not an SVG: {}", path.display());
                continue;
            }

            if let Some(bytes) = file.bytes.as_ref() {
                match String::from_utf8(bytes.to_vec()) {
                    Ok(svg) => {
                        let name = if file.name.trim().is_empty() {
                            "dropped.svg".to_string()
                        } else {
                            file.name.clone()
                        };
                        self.load_svg_text(svg, name, None, ctx);
                        return;
                    }
                    Err(_) => {
                        self.status = "Dropped SVG was not valid UTF-8 text.".to_string();
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

        let dragging_files = ctx.input(|input| !input.raw.hovered_files.is_empty());

        egui::TopBottomPanel::top("toolbar")
            .exact_height(58.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("SRNG Studio").strong().size(20.0));
                    ui.separator();
                    if ui.button("Open SVG").on_hover_text("Choose an SVG file").clicked() {
                        self.open_dialog(ctx);
                    }
                    if ui
                        .add_enabled(!self.svg_source.is_empty(), egui::Button::new("Convert"))
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let name = if self.svg_source.is_empty() {
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
                            let text = self.diagnostics_text();
                            if ui.button("Copy diagnostics").clicked() {
                                self.copy_text(ctx, "diagnostics", text);
                            }
                        }
                        InfoTab::Metadata => {
                            let text = self.metadata_text();
                            if ui.button("Copy metadata").clicked() {
                                self.copy_text(ctx, "metadata", text);
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
                                        let level = match diagnostic.severity.as_str() {
                                            "error" => egui::RichText::new(format!(
                                                "{}[{}]",
                                                diagnostic.severity, diagnostic.code
                                            ))
                                            .strong(),
                                            "warning" => egui::RichText::new(format!(
                                                "{}[{}]",
                                                diagnostic.severity, diagnostic.code
                                            ))
                                            .strong(),
                                            _ => egui::RichText::new(format!(
                                                "{}[{}]",
                                                diagnostic.severity, diagnostic.code
                                            )),
                                        };
                                        ui.monospace(level.text());
                                        ui.label(&diagnostic.message);
                                    });
                                }
                            }
                        }
                        InfoTab::Metadata => {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.metadata_text())
                                    .code_editor()
                                    .interactive(false)
                                    .desired_width(f32::INFINITY),
                            );
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);

            if self.svg_source.is_empty() {
                let available = ui.available_size();
                ui.allocate_ui_with_layout(
                    available,
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space((available.y * 0.28).max(30.0));
                        ui.label(egui::RichText::new("Drop an SVG here").strong().size(26.0));
                        ui.add_space(8.0);
                        ui.label("Drag from your file manager, or use Open SVG.");
                        ui.add_space(16.0);
                        if ui.button("Open SVG").clicked() {
                            self.open_dialog(ctx);
                        }
                    },
                );
                return;
            }

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
                            panel_header(ui, "Original SVG", Some("Source rendered by resvg"));
                            preview_panel(
                                ui,
                                self.original_texture.as_ref(),
                                self.original_image.as_ref(),
                                "SVG preview unavailable.",
                            );
                        },
                    );
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        egui::vec2(half, preview_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            panel_header(ui, "SRNG Render", Some("Rendered by the SRNG CPU renderer"));
                            preview_panel(
                                ui,
                                self.rendered_texture.as_ref(),
                                self.rendered_image.as_ref(),
                                "SRNG preview unavailable.",
                            );
                        },
                    );
                },
            );

            ui.separator();
            ui.horizontal(|ui| {
                panel_header(ui, "Generated SRNG", Some("Editable before re-rendering"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Copy SRNG").clicked() {
                        self.copy_text(ctx, "SRNG", self.srng_source.clone());
                    }
                    if ui.button("Copy SVG source").clicked() {
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
            let screen = ctx.screen_rect();
            painter.rect_filled(screen, 8.0, egui::Color32::from_black_alpha(180));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop SVG to open",
                egui::FontId::proportional(30.0),
                egui::Color32::WHITE,
            );
        }
    }
}

fn panel_header(ui: &mut egui::Ui, title: &str, subtitle: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().size(16.0));
        if let Some(subtitle) = subtitle {
            ui.small(subtitle);
        }
    });
}

fn load_texture(ctx: &egui::Context, name: &str, image: &RgbaImage) -> egui::TextureHandle {
    let color = egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.pixels);
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

fn preview_panel(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    image: Option<&RgbaImage>,
    empty: &str,
) {
    ui.add_space(4.0);
    let frame = egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8));
    frame.show(ui, |ui| {
        let available = ui.available_size();
        let Some(texture) = texture else {
            ui.allocate_ui_with_layout(
                available,
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(empty);
                },
            );
            return;
        };
        let Some(image) = image else {
            return;
        };
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let available = ui.available_size();
            let natural = egui::vec2(image.width as f32, image.height as f32);
            let scale = (available.x / natural.x)
                .min((available.y - 24.0).max(1.0) / natural.y)
                .min(1.0)
                .max(0.03);
            let size = natural * scale;
            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                ui.small(format!("{} × {} px", image.width, image.height));
            });
        });
    });
}
