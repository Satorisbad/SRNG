pub(super) fn parse_number(value: &str) -> Option<f64> {
    value.trim().trim_end_matches("px").parse().ok()
}

pub(super) fn resolve_coord(raw: &str, units: &str, origin: f64, extent: f64) -> f64 {
    let raw = raw.trim();
    if let Some(percent) = raw
        .strip_suffix('%')
        .and_then(|value| value.parse::<f64>().ok())
    {
        return if units == "objectBoundingBox" {
            origin + extent * percent / 100.0
        } else {
            percent / 100.0
        };
    }

    let value = raw.trim_end_matches("px").parse::<f64>().unwrap_or(0.0);
    if units == "objectBoundingBox" {
        origin + extent * value
    } else {
        value
    }
}

pub(super) fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return trimmed.to_string();
    };

    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub(super) fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

pub(super) fn fmt(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{}", value as i64)
    } else {
        format!("{value:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub(super) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_bbox_percent_coordinate_resolves() {
        assert!((resolve_coord("50%", "objectBoundingBox", 10.0, 20.0) - 20.0).abs() < 1e-9);
    }
}
