use srng::runtime::{execute_file, RuntimeOptions};
use std::env;
use std::fs;
use std::path::PathBuf;

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

    let mut output = None;
    let mut stdout = false;
    let mut options = RuntimeOptions::default();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-o" | "--output" => output = Some(PathBuf::from(required_value(&mut args, &argument))),
            "--stdout" => stdout = true,
            "--viewport" => {
                let value = required_value(&mut args, &argument);
                let Some((width, height)) = value.split_once('x') else {
                    fail("--viewport must use WIDTHxHEIGHT");
                };
                options.viewport_width = parse_positive(width, "viewport width");
                options.viewport_height = parse_positive(height, "viewport height");
            }
            "--dpi" => options.dpi = parse_positive(&required_value(&mut args, &argument), "DPI"),
            "--no-resolve" => options.resolve_references = false,
            unknown => fail(&format!("unknown argument `{unknown}`")),
        }
    }

    let scene = execute_file(&input, &options).unwrap_or_else(|error| fail(&error.to_string()));
    let json = scene
        .to_json_pretty()
        .unwrap_or_else(|error| fail(&error.to_string()));
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

    if stdout {
        println!("{json}");
    } else {
        let path = output.unwrap_or_else(|| PathBuf::from(format!("{input}.scene.json")));
        fs::write(&path, json).unwrap_or_else(|error| {
            fail(&format!("could not write `{}`: {error}", path.display()))
        });
        println!("ran {} -> {}", input, path.display());
    }

    if scene.has_errors() {
        std::process::exit(1);
    }
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
    eprintln!("srngr: {message}");
    std::process::exit(2);
}

fn usage() {
    eprintln!("Usage: srngr <input.srng|input.json> [-o scene.json] [--stdout] [--viewport WIDTHxHEIGHT] [--dpi DPI] [--no-resolve]");
}
