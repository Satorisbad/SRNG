use crate::{PreparedScene, RevisionGate};
use srng::runtime::Scene;

/// Provides a deterministic, dependency-free text fallback before the SVG
/// compatibility passes. Imported text remains semantic SRNG (`content`,
/// font-size, text-anchor, etc.), while this pass supplies simple vector
/// geometry when no pre-shaped outline data is present.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();
    for node in &mut normalized.nodes {
        if !node.active || node.kind != "text" || node.properties.contains_key("data") {
            continue;
        }
        let content = node
            .properties
            .get("content")
            .map(|v| unquote(v))
            .unwrap_or_default();
        if content.is_empty() {
            continue;
        }
        let size = node
            .properties
            .get("font-size")
            .map(|v| unquote(v))
            .and_then(|v| parse_number(&v))
            .unwrap_or(16.0)
            .max(1.0);
        let anchor = node
            .properties
            .get("text-anchor")
            .map(|v| unquote(v))
            .unwrap_or_else(|| "start".to_string());
        let baseline_x = node.geometry.x.unwrap_or(0.0);
        let baseline_y = node.geometry.y.unwrap_or(size);
        let (path, width) = bitmap_text_path(&content, baseline_x, baseline_y, size, &anchor);
        if path.is_empty() {
            continue;
        }
        node.properties.insert("data".into(), quote(&path));
        node.geometry.x = Some(match anchor.as_str() {
            "middle" => baseline_x - width / 2.0,
            "end" => baseline_x - width,
            _ => baseline_x,
        });
        node.geometry.y = Some(baseline_y - size);
        node.geometry.width = Some(width);
        node.geometry.height = Some(size);
    }
    crate::semantic_v6::prepare_scene(&normalized, revision, gate)
}

fn bitmap_text_path(text: &str, x: f64, baseline_y: f64, size: f64, anchor: &str) -> (String, f64) {
    let cell = size / 7.0;
    let advance = cell * 6.0;
    let chars = text.chars().collect::<Vec<_>>();
    let width = chars.len() as f64 * advance;
    let start_x = match anchor {
        "middle" => x - width / 2.0,
        "end" => x - width,
        _ => x,
    };
    let top = baseline_y - size;
    let mut out = String::new();
    for (index, ch) in chars.into_iter().enumerate() {
        let rows = glyph(ch);
        let gx = start_x + index as f64 * advance;
        for (row, bits) in rows.iter().copied().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                let x0 = gx + col as f64 * cell;
                let y0 = top + row as f64 * cell;
                let x1 = x0 + cell;
                let y1 = y0 + cell;
                out.push_str(&format!(
                    "M {} {} H {} V {} H {} Z ",
                    fmt(x0),
                    fmt(y0),
                    fmt(x1),
                    fmt(y1),
                    fmt(x0)
                ));
            }
        }
    }
    (out.trim().to_string(), width)
}

fn glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0b01110,0b10001,0b10001,0b11111,0b10001,0b10001,0b10001],
        'B' => [0b11110,0b10001,0b10001,0b11110,0b10001,0b10001,0b11110],
        'C' => [0b01111,0b10000,0b10000,0b10000,0b10000,0b10000,0b01111],
        'D' => [0b11110,0b10001,0b10001,0b10001,0b10001,0b10001,0b11110],
        'E' => [0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b11111],
        'F' => [0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b10000],
        'G' => [0b01111,0b10000,0b10000,0b10111,0b10001,0b10001,0b01111],
        'H' => [0b10001,0b10001,0b10001,0b11111,0b10001,0b10001,0b10001],
        'I' => [0b11111,0b00100,0b00100,0b00100,0b00100,0b00100,0b11111],
        'J' => [0b00111,0b00010,0b00010,0b00010,0b10010,0b10010,0b01100],
        'K' => [0b10001,0b10010,0b10100,0b11000,0b10100,0b10010,0b10001],
        'L' => [0b10000,0b10000,0b10000,0b10000,0b10000,0b10000,0b11111],
        'M' => [0b10001,0b11011,0b10101,0b10101,0b10001,0b10001,0b10001],
        'N' => [0b10001,0b11001,0b10101,0b10011,0b10001,0b10001,0b10001],
        'O' => [0b01110,0b10001,0b10001,0b10001,0b10001,0b10001,0b01110],
        'P' => [0b11110,0b10001,0b10001,0b11110,0b10000,0b10000,0b10000],
        'Q' => [0b01110,0b10001,0b10001,0b10001,0b10101,0b10010,0b01101],
        'R' => [0b11110,0b10001,0b10001,0b11110,0b10100,0b10010,0b10001],
        'S' => [0b01111,0b10000,0b10000,0b01110,0b00001,0b00001,0b11110],
        'T' => [0b11111,0b00100,0b00100,0b00100,0b00100,0b00100,0b00100],
        'U' => [0b10001,0b10001,0b10001,0b10001,0b10001,0b10001,0b01110],
        'V' => [0b10001,0b10001,0b10001,0b10001,0b10001,0b01010,0b00100],
        'W' => [0b10001,0b10001,0b10001,0b10101,0b10101,0b10101,0b01010],
        'X' => [0b10001,0b10001,0b01010,0b00100,0b01010,0b10001,0b10001],
        'Y' => [0b10001,0b10001,0b01010,0b00100,0b00100,0b00100,0b00100],
        'Z' => [0b11111,0b00001,0b00010,0b00100,0b01000,0b10000,0b11111],
        '0' => [0b01110,0b10001,0b10011,0b10101,0b11001,0b10001,0b01110],
        '1' => [0b00100,0b01100,0b00100,0b00100,0b00100,0b00100,0b01110],
        '2' => [0b01110,0b10001,0b00001,0b00010,0b00100,0b01000,0b11111],
        '3' => [0b11110,0b00001,0b00001,0b01110,0b00001,0b00001,0b11110],
        '4' => [0b00010,0b00110,0b01010,0b10010,0b11111,0b00010,0b00010],
        '5' => [0b11111,0b10000,0b10000,0b11110,0b00001,0b00001,0b11110],
        '6' => [0b01110,0b10000,0b10000,0b11110,0b10001,0b10001,0b01110],
        '7' => [0b11111,0b00001,0b00010,0b00100,0b01000,0b01000,0b01000],
        '8' => [0b01110,0b10001,0b10001,0b01110,0b10001,0b10001,0b01110],
        '9' => [0b01110,0b10001,0b10001,0b01111,0b00001,0b00001,0b01110],
        ' ' => [0,0,0,0,0,0,0],
        '-' => [0,0,0,0b11111,0,0,0],
        '_' => [0,0,0,0,0,0,0b11111],
        '.' => [0,0,0,0,0,0,0b00100],
        ':' => [0,0b00100,0,0,0b00100,0,0],
        '/' => [0b00001,0b00010,0b00100,0b01000,0b10000,0,0],
        _ => [0b11111,0b10001,0b00101,0b00100,0b10100,0b10001,0b11111],
    }
}

fn parse_number(value: &str) -> Option<f64> {
    value.trim().trim_end_matches("px").parse().ok()
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
    else {
        return value.to_string();
    };
    inner
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn fmt(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{}", value as i64)
    } else {
        format!("{value:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_text_generates_vector_geometry() {
        let (path, width) = bitmap_text_path("SRNG", 2.0, 18.0, 16.0, "start");
        assert!(path.contains('M'));
        assert!(width > 0.0);
    }

    #[test]
    fn middle_anchor_offsets_path() {
        let (start, _) = bitmap_text_path("A", 20.0, 18.0, 14.0, "start");
        let (middle, _) = bitmap_text_path("A", 20.0, 18.0, 14.0, "middle");
        assert_ne!(start, middle);
    }
}
