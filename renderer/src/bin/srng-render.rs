use srng::runtime::{execute_file, RuntimeOptions, Scene};
use srng_renderer::{cpu, prepare_scene, RevisionGate};
use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

fn main() {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        usage();
        std::process::exit(2);
    };
    if matches!(input.as_str(), "-h" | "--help") {
        usage();
        return;
    }

    let mut output: Option<PathBuf> = None;
    let mut options = RuntimeOptions::default();
    let mut explicit_viewport = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                output = Some(PathBuf::from(required_value(&mut args, &arg)));
            }
            "--viewport" => {
                let value = required_value(&mut args, &arg);
                let Some((width, height)) = value.split_once('x') else {
                    fail("--viewport must use WIDTHxHEIGHT");
                };
                options.viewport_width = parse_positive(width, "viewport width");
                options.viewport_height = parse_positive(height, "viewport height");
                explicit_viewport = true;
            }
            "--dpi" => {
                options.dpi = parse_positive(&required_value(&mut args, &arg), "DPI");
            }
            other => fail(&format!("unknown argument `{other}`")),
        }
    }

    let mut scene = execute_file(&input, &options).unwrap_or_else(|error| fail(&error.to_string()));
    if !explicit_viewport {
        if let Some((width, height)) = canvas_size(&scene) {
            if width > 0.0 && height > 0.0
                && (width != options.viewport_width || height != options.viewport_height)
            {
                options.viewport_width = width;
                options.viewport_height = height;
                scene = execute_file(&input, &options)
                    .unwrap_or_else(|error| fail(&error.to_string()));
            }
        }
    }

    for diagnostic in &scene.diagnostics {
        eprintln!(
            "{}:{}:{}: {}[{}]: {}",
            diagnostic.source,
            diagnostic.line,
            diagnostic.column,
            diagnostic.severity,
            diagnostic.code,
            diagnostic.message
        );
    }
    if scene.has_errors() {
        fail("scene contains runtime errors; PNG was not written");
    }

    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene, revision, &gate);
    let rendered = cpu::render(&prepared);

    for diagnostic in &rendered.diagnostics {
        let declaration = diagnostic
            .declaration
            .as_deref()
            .map(|value| format!(" [{value}]"))
            .unwrap_or_default();
        eprintln!(
            "renderer: {}[{}]{}: {}",
            diagnostic.severity, diagnostic.code, declaration, diagnostic.message
        );
    }
    if rendered.diagnostics.iter().any(|d| d.severity == "error") {
        fail("renderer reported errors; PNG was not written");
    }

    let output = output.unwrap_or_else(|| default_output(&input));
    write_png(&output, rendered.width, rendered.height, &rendered.pixels)
        .unwrap_or_else(|error| fail(&format!("could not write `{}`: {error}", output.display())));

    println!(
        "rendered {} -> {} ({}x{})",
        input,
        output.display(),
        rendered.width,
        rendered.height
    );
}

fn canvas_size(scene: &Scene) -> Option<(f64, f64)> {
    scene
        .nodes
        .iter()
        .find(|node| node.active && node.kind == "canvas")
        .and_then(|node| Some((node.geometry.width?, node.geometry.height?)))
}

fn write_png(path: &Path, width: u16, height: u16, pixels: &[u8]) -> Result<(), String> {
    let expected = width as usize * height as usize * 4;
    if pixels.len() != expected {
        return Err(format!(
            "renderer returned {} bytes, expected {expected}",
            pixels.len()
        ));
    }

    let file = File::create(path).map_err(|error| error.to_string())?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(pixels)
        .map_err(|error| error.to_string())
}

fn default_output(input: &str) -> PathBuf {
    let path = Path::new(input);
    path.with_extension("png")
}

fn required_value(args: &mut impl Iterator<Item = String>, option: &str) -> String {
    args.next()
        .unwrap_or_else(|| fail(&format!("expected a value after {option}")))
}

fn parse_positive(value: &str, label: &str) -> f64 {
    value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number > 0.0)
        .unwrap_or_else(|| fail(&format!("{label} must be a positive number")))
}

fn fail(message: &str) -> ! {
    eprintln!("srng-render: {message}");
    std::process::exit(1);
}

fn usage() {
    eprintln!(
        "Usage: srng-render <input.srng|input.json> [-o output.png] [--viewport WIDTHxHEIGHT] [--dpi DPI]"
    );
}
