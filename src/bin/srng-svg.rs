use srng::svg::{import_svg, ImportOptions};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        usage();
        std::process::exit(2);
    };

    if input == "--help" || input == "-h" {
        usage();
        return;
    }

    let mut output: Option<PathBuf> = None;
    let mut stdout = false;
    let mut strict = false;
    let mut file_id: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                let Some(path) = args.next() else {
                    eprintln!("srng-svg: expected a path after {arg}");
                    std::process::exit(2);
                };
                output = Some(PathBuf::from(path));
            }
            "--stdout" => stdout = true,
            "--strict" => strict = true,
            "--file-id" => {
                let Some(value) = args.next() else {
                    eprintln!("srng-svg: expected a value after --file-id");
                    std::process::exit(2);
                };
                file_id = Some(value);
            }
            other => {
                eprintln!("srng-svg: unknown argument `{other}`");
                usage();
                std::process::exit(2);
            }
        }
    }

    let source = match fs::read_to_string(&input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("srng-svg: could not read `{input}`: {error}");
            std::process::exit(1);
        }
    };

    let source_name = Path::new(&input)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&input);
    let options = ImportOptions {
        file_id,
        ..ImportOptions::default()
    };
    let imported = import_svg(&source, source_name, &options);

    for diagnostic in &imported.diagnostics {
        let element = diagnostic
            .element
            .as_deref()
            .map(|id| format!(" [{id}]"))
            .unwrap_or_default();
        eprintln!(
            "{input}: {}[{}]{}: {}",
            diagnostic.severity, diagnostic.code, element, diagnostic.message
        );
    }

    if stdout {
        print!("{}", imported.source);
    } else {
        let output = output.unwrap_or_else(|| default_output(&input));
        if let Err(error) = fs::write(&output, &imported.source) {
            eprintln!("srng-svg: could not write `{}`: {error}", output.display());
            std::process::exit(1);
        }
        println!("imported {} -> {}", input, output.display());
    }

    let strict_failure = strict
        && imported
            .diagnostics
            .iter()
            .any(|d| d.severity == "warning" || d.severity == "error");
    if imported.has_errors() || strict_failure {
        std::process::exit(1);
    }
}

fn default_output(input: &str) -> PathBuf {
    let path = Path::new(input);
    if path.extension().is_some_and(|extension| extension == "svg") {
        path.with_extension("srng")
    } else {
        PathBuf::from(format!("{input}.srng"))
    }
}

fn usage() {
    eprintln!(
        "Usage: srng-svg <input.svg> [-o output.srng] [--stdout] [--strict] [--file-id ID]"
    );
}
