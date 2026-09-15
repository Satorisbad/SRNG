use crate::ast::{Declaration, Document, Property};

pub fn emit_json(document: &Document, source_name: &str) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    field(&mut out, 1, "format", "SRNG-IR", true);
    field(&mut out, 1, "version", &document.version, true);
    field(&mut out, 1, "source", source_name, true);
    match &document.file_id {
        Some(id) => field(&mut out, 1, "file_id", id, true),
        None => out.push_str("  \"file_id\": null,\n"),
    }

    out.push_str("  \"declarations\": [\n");
    for (index, decl) in document.declarations.iter().enumerate() {
        out.push_str(&emit_decl(decl, 2));
        if index + 1 != document.declarations.len() { out.push(','); }
        out.push('\n');
    }
    out.push_str("  ],\n");

    out.push_str("  \"diagnostics\": [\n");
    for (index, diag) in document.diagnostics.iter().enumerate() {
        out.push_str("    {\n");
        field(&mut out, 3, "severity", diag.severity.as_str(), true);
        field(&mut out, 3, "code", diag.code, true);
        field(&mut out, 3, "message", &diag.message, true);
        out.push_str(&format!("      \"line\": {},\n", diag.line));
        out.push_str(&format!("      \"column\": {}\n", diag.column));
        out.push_str("    }");
        if index + 1 != document.diagnostics.len() { out.push(','); }
        out.push('\n');
    }
    out.push_str("  ]\n}");
    out
}

fn emit_decl(decl: &Declaration, level: usize) -> String {
    let mut out = String::new();
    let indent = "  ".repeat(level);
    out.push_str(&format!("{indent}{{\n"));
    match decl {
        Declaration::Unit(unit) => {
            field(&mut out, level + 1, "type", "unit", true);
            field(&mut out, level + 1, "name", &unit.name, true);
            out.push_str(&format!("{}\"scale\": {},\n", "  ".repeat(level + 1), unit.scale));
            field(&mut out, level + 1, "base", &unit.base, false);
        }
        Declaration::Node(node) => {
            field(&mut out, level + 1, "type", "node", true);
            field(&mut out, level + 1, "kind", &node.kind, true);
            field(&mut out, level + 1, "id", &node.id, true);
            emit_properties(&mut out, level + 1, &node.properties);
        }
        Declaration::Relation(relation) => {
            field(&mut out, level + 1, "type", "relation", true);
            field(&mut out, level + 1, "from", &relation.from, true);
            field(&mut out, level + 1, "to", &relation.to, true);
            emit_properties(&mut out, level + 1, &relation.properties);
        }
        Declaration::Reference(reference) => {
            field(&mut out, level + 1, "type", "reference", true);
            field(&mut out, level + 1, "id", &reference.id, true);
            field(&mut out, level + 1, "target", &reference.target, true);
            let (source_file, target_id) = split_reference(&reference.target);
            field(&mut out, level + 1, "source_file", source_file, true);
            field(&mut out, level + 1, "target_id", target_id, true);
            emit_properties(&mut out, level + 1, &reference.properties);
        }
        Declaration::Animation(animation) => {
            field(&mut out, level + 1, "type", "animation", true);
            field(&mut out, level + 1, "id", &animation.id, true);
            emit_properties(&mut out, level + 1, &animation.properties);
        }
    }
    out.push_str(&format!("\n{indent}}}"));
    out
}

fn emit_properties(out: &mut String, level: usize, properties: &[Property]) {
    out.push_str(&format!("{}\"properties\": {{\n", "  ".repeat(level)));
    for (index, property) in properties.iter().enumerate() {
        out.push_str(&format!(
            "{}\"{}\": \"{}\"{}\n",
            "  ".repeat(level + 1),
            escape(&property.name),
            escape(&property.value),
            if index + 1 == properties.len() { "" } else { "," }
        ));
    }
    out.push_str(&format!("{}}}", "  ".repeat(level)));
}

fn field(out: &mut String, level: usize, key: &str, value: &str, comma: bool) {
    out.push_str(&format!(
        "{}\"{}\": \"{}\"{}\n",
        "  ".repeat(level), escape(key), escape(value), if comma { "," } else { "" }
    ));
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

fn split_reference(target: &str) -> (&str, &str) {
    match target.rsplit_once('#') { Some((source, id)) => (source, id), None => (target, "") }
}
