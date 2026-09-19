use std::env;
use std::fs;
use std::path::PathBuf;

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
    let mut check = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                let Some(path) = args.next() else {
                    eprintln!("srngc: expected a path after {arg}");
                    std::process::exit(2);
                };
                output = Some(PathBuf::from(path));
            }
            "--stdout" => stdout = true,
            "--check" => check = true,
            other => {
                eprintln!("srngc: unknown argument `{other}`");
                usage();
                std::process::exit(2);
            }
        }
    }

    if check && (stdout || output.is_some()) {
        eprintln!("srngc: --check cannot be combined with --stdout or --output");
        std::process::exit(2);
    }

    let source = match fs::read_to_string(&input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("srngc: could not read `{input}`: {error}");
            std::process::exit(1);
        }
    };

    let document = srng::compile(&source);
    let json = srng::ir::emit_json(&document, &input);

    for diagnostic in &document.diagnostics {
        eprintln!(
            "{}:{}:{}: {}[{}]: {}",
            input,
            diagnostic.line,
            diagnostic.column,
            diagnostic.severity.as_str(),
            diagnostic.code,
            diagnostic.message
        );
    }

    let has_errors = document
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, srng::diagnostic::Severity::Error));

    if check {
        if has_errors {
            std::process::exit(1);
        }
        println!("valid {input}");
        return;
    }

    if stdout {
        println!("{json}");
    } else {
        let output = output.unwrap_or_else(|| PathBuf::from(format!("{input}.json")));
        if let Err(error) = fs::write(&output, json) {
            eprintln!("srngc: could not write `{}`: {error}", output.display());
            std::process::exit(1);
        }
        println!("compiled {} -> {}", input, output.display());
    }

    if has_errors {
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!("Usage: srngc <input.srng> [-o output.json] [--stdout] [--check]");
}
