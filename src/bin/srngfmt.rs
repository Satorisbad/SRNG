use std::env;
use std::fs;
use std::path::Path;

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

    let mut write = false;
    let mut check = false;
    for arg in args {
        match arg.as_str() {
            "-w" | "--write" => write = true,
            "--check" => check = true,
            other => {
                eprintln!("srngfmt: unknown argument `{other}`");
                usage();
                std::process::exit(2);
            }
        }
    }
    if write && check {
        eprintln!("srngfmt: --write and --check cannot be used together");
        std::process::exit(2);
    }

    let source = match fs::read_to_string(&input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("srngfmt: could not read `{input}`: {error}");
            std::process::exit(1);
        }
    };
    let formatted = format_srng(&source);

    if check {
        if formatted != source {
            eprintln!("srngfmt: `{input}` is not formatted");
            std::process::exit(1);
        }
        return;
    }

    if write {
        if let Err(error) = fs::write(Path::new(&input), formatted) {
            eprintln!("srngfmt: could not write `{input}`: {error}");
            std::process::exit(1);
        }
    } else {
        print!("{formatted}");
    }
}

fn format_srng(source: &str) -> String {
    let mut out = String::new();
    let mut indent = 0usize;

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() {
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }

        let (opens, closes, starts_with_close) = brace_shape(line);
        if starts_with_close {
            indent = indent.saturating_sub(1);
        }

        out.push_str(&"    ".repeat(indent));
        out.push_str(line);
        out.push('\n');

        let already_consumed_close = usize::from(starts_with_close);
        indent = indent
            .saturating_add(opens)
            .saturating_sub(closes.saturating_sub(already_consumed_close));
    }

    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn brace_shape(line: &str) -> (usize, usize, bool) {
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            break;
        }
        match ch {
            '{' => opens += 1,
            '}' => closes += 1,
            _ => {}
        }
    }

    (opens, closes, line.starts_with('}'))
}

fn usage() {
    eprintln!("Usage: srngfmt <input.srng> [-w|--write] [--check]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_blocks_without_touching_strings_or_comments() {
        let source = "srng 0.1;\nrect a {\ncontent: \"} {\"; // }\nfill: #fff;\n}\n";
        let expected = "srng 0.1;\nrect a {\n    content: \"} {\"; // }\n    fill: #fff;\n}\n";
        assert_eq!(format_srng(source), expected);
    }

    #[test]
    fn formatting_is_idempotent() {
        let source = "srng 0.1;\n\nrect a {\n    fill: #fff;\n}\n";
        let once = format_srng(source);
        assert_eq!(format_srng(&once), once);
    }
}
