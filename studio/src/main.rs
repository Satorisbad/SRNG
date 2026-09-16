use eframe::egui;
use srng_studio::{convert_svg, render_srng, write_png, RgbaImage, StudioDiagnostic};
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SRNG SVG Studio",
        options,
        Box::new(move |cc| Ok(Box::new(StudioApp::new(cc, initial.clone())))),
    )
}

struct StudioApp {
    path: Option<PathBuf>,
    svg_source: String,
    srng_source: String,
    diagnostics: Vec<StudioDiagnostic>,
    original_image: Option<RgbaImage>,
    rendered_image: Option<RgbaImage>,
    original_texture: Option<egui::TextureHandle>,
    rendered_texture: Option<egui::TextureHandle>,
    status: String,
}

impl StudioApp {
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            path: None,
            svg_source: String::new(),
            srng_source: String::new(),
            diagnostics: Vec::new(),
            original_image: None,
            rendered_image: None,
            original_texture: None,
            rendered_texture: None,
            status: "Open an SVG file to begin.".to_string(),
        };
        if let Some(path) = initial {
            app.load_path(&path, &cc.egui_ctx);
        }
        app
    }

    fn load_path(&mut self, path: &Path, ctx: &egui::Context) {
        match fs::read_to_string(path) {
            Ok(svg) => {
                self.path = Some(path.to_path_buf());
                self.svg_source = svg;
                self.convert(ctx);
            }
            Err(error) => {
                self.status = format!("Could not open {}: {error}", path.display());
            }
        }
    }

    fn convert(&mut self, ctx: &egui::Context) {
        let source_name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|p| p.to_str())
            .unwrap_or("untitled.svg");
        let outcome = convert_svg(&self.svg_source, source_name);
        self.srng_source = outcome.srng;
        self.diagnostics = outcome.diagnostics;
        self.original_image = outcome.original;
        self.rendered_image = outcome.rendered;
        self.refresh_textures(ctx);
        self.status = if self.diagnostics.iter().any(|d| d.severity == "error") {
            "Conversion completed with errors.".to_string()
        } else if self.diagnostics.iter().any(|d| d.severity == "warning") {
            "Conversion completed with warnings.".to_string()
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
            "Rendered current SRNG text.".to_string()
        } else {
            "SRNG render failed; see diagnostics.".to_string()
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
        if let Some(path) = &self.path {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                dialog = dialog.set_file_name(format!("{stem}.srng"));
            }
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
        if let Some(path) = &self.path {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                dialog = dialog.set_file_name(format!("{stem}-srng.png"));
            }
        }
        if let Some(path) = dialog.save_file() {
            match write_png(&path, image) {
                Ok(()) => self.status = format!("Saved {}", path.display()),
                Err(error) => self.status = format!("Could not save {}: {error}", path.display()),
            }
        }
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        if let Some(path) = dropped.into_iter().find_map(|file| file.path) {
            self.load_path(&path, ctx);
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open SVG").clicked() {
                    self.open_dialog(ctx);
                }
                if ui
                    .add_enabled(!self.svg_source.is_empty(), egui::Button::new("Convert"))
                    .clicked()
                {
                    self.convert(ctx);
                }
                if ui
                    .add_enabled(!self.srng_source.is_empty(), egui::Button::new("Render SRNG"))
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
                ui.label(&self.status);
            });
        });

        egui::TopBottomPanel::bottom("diagnostics")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                ui.heading("Diagnostics");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.diagnostics.is_empty() {
                        ui.label("No diagnostics.");
                    } else {
                        for diagnostic in &self.diagnostics {
                            ui.horizontal_wrapped(|ui| {
                                ui.monospace(format!("{}[{}]", diagnostic.severity, diagnostic.code));
                                ui.label(&diagnostic.message);
                            });
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(3, |columns| {
                columns[0].heading("Original SVG");
                preview_panel(
                    &mut columns[0],
                    self.original_texture.as_ref(),
                    self.original_image.as_ref(),
                    "Open or drop an SVG here.",
                );

                columns[1].heading("Generated SRNG");
                columns[1].add(
                    egui::TextEdit::multiline(&mut self.srng_source)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(40),
                );

                columns[2].heading("SRNG Render");
                preview_panel(
                    &mut columns[2],
                    self.rendered_texture.as_ref(),
                    self.rendered_image.as_ref(),
                    "Convert the SVG to render SRNG.",
                );
            });
        });
    }
}

fn load_texture(ctx: &egui::Context, name: &str, image: &RgbaImage) -> egui::TextureHandle {
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width, image.height],
        &image.pixels,
    );
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

fn preview_panel(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    image: Option<&RgbaImage>,
    empty: &str,
) {
    ui.separator();
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.label(empty);
        });
        return;
    };
    let Some(image) = image else {
        return;
    };
    egui::ScrollArea::both().show(ui, |ui| {
        let available = ui.available_size();
        let natural = egui::vec2(image.width as f32, image.height as f32);
        let scale = (available.x / natural.x)
            .min(available.y / natural.y)
            .min(1.0)
            .max(0.05);
        let size = natural * scale;
        ui.add(egui::Image::new(texture).fit_to_exact_size(size));
        ui.small(format!("{} × {} px", image.width, image.height));
    });
}
