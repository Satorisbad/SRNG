use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub dpi: f64,
    pub resolve_references: bool,
    pub max_reference_depth: usize,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            viewport_width: 1920.0,
            viewport_height: 1080.0,
            dpi: 96.0,
            resolve_references: true,
            max_reference_depth: 32,
        }
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    Io { path: PathBuf, source: std::io::Error },
    InvalidIr(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "could not read `{}`: {source}", path.display()),
            Self::InvalidIr(message) => write!(f, "invalid SRNG IR: {message}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub format: String,
    pub version: String,
    pub source: String,
    pub file_id: Option<String>,
    pub viewport: Viewport,
    pub nodes: Vec<SceneNode>,
    pub relations: Vec<SceneRelation>,
    pub references: Vec<SceneReference>,
    pub animations: Vec<SceneAnimation>,
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

impl Scene {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|diagnostic| diagnostic.severity == "error")
    }

    pub fn to_json_pretty(&self) -> Result<String, RuntimeError> {
        serde_json::to_string_pretty(self).map_err(|error| RuntimeError::InvalidIr(error.to_string()))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    pub dpi: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNode {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub properties: BTreeMap<String, String>,
    pub geometry: Geometry,
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Geometry {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneRelation {
    pub from: String,
    pub to: String,
    pub kind: Option<String>,
    pub properties: BTreeMap<String, String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneReference {
    pub id: String,
    pub source_file: String,
    pub target_id: String,
    pub provenance: String,
    pub resolved: bool,
    pub resolved_kind: Option<String>,
    pub properties: BTreeMap<String, String>,
    pub geometry: Geometry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneAnimation {
    pub id: String,
    pub reference: Option<String>,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub source: String,
    pub declaration: Option<String>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Deserialize)]
struct IrDocument {
    format: String,
    version: String,
    source: String,
    file_id: Option<String>,
    #[serde(default)]
    declarations: Vec<Value>,
    #[serde(default)]
    diagnostics: Vec<IrDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct IrDiagnostic {
    severity: String,
    code: String,
    message: String,
    #[serde(default)]
    line: usize,
    #[serde(default)]
    column: usize,
}

#[derive(Debug, Clone)]
struct UnitDefinition {
    scale: f64,
    base: String,
}

pub fn execute_json(ir_json: &str, options: &RuntimeOptions) -> Result<Scene, RuntimeError> {
    execute_json_from(ir_json, None, options)
}

pub fn execute_file(path: impl AsRef<Path>, options: &RuntimeOptions) -> Result<Scene, RuntimeError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|source| RuntimeError::Io { path: path.to_path_buf(), source })?;
    let ir_json = if path.extension().and_then(|value| value.to_str()) == Some("srng") {
        crate::compile_to_json(&text, &path.to_string_lossy())
    } else {
        text
    };
    execute_json_from(&ir_json, Some(path), options)
}

fn execute_json_from(ir_json: &str, input_path: Option<&Path>, options: &RuntimeOptions) -> Result<Scene, RuntimeError> {
    validate_options(options)?;
    let ir: IrDocument = serde_json::from_str(ir_json).map_err(|error| RuntimeError::InvalidIr(error.to_string()))?;
    if ir.format != "SRNG-IR" {
        return Err(RuntimeError::InvalidIr(format!("expected format `SRNG-IR`, found `{}`", ir.format)));
    }

    let mut diagnostics = ir.diagnostics.iter().map(|diagnostic| RuntimeDiagnostic {
        severity: diagnostic.severity.clone(),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        source: ir.source.clone(),
        declaration: None,
        line: diagnostic.line,
        column: diagnostic.column,
    }).collect::<Vec<_>>();

    let units = collect_units(&ir.declarations, &ir.source, &mut diagnostics);
    let mut nodes = Vec::new();
    let mut relations = Vec::new();
    let mut references = Vec::new();
    let mut animations = Vec::new();

    for declaration in &ir.declarations {
        match string_field(declaration, "type").unwrap_or("") {
            "unit" => {}
            "node" => {
                if let Some(node) = build_node(declaration, &ir.source, &units, options, &mut diagnostics) {
                    nodes.push(node);
                }
            }
            "relation" => {
                let properties = properties(declaration);
                relations.push(SceneRelation {
                    from: string_field(declaration, "from").unwrap_or("").to_string(),
                    to: string_field(declaration, "to").unwrap_or("").to_string(),
                    kind: properties.get("kind").map(|value| unquote(value)),
                    properties,
                    active: false,
                });
            }
            "reference" => {
                if let Some(reference) = build_reference(declaration, &ir.source, &units, options, &mut diagnostics) {
                    references.push(reference);
                }
            }
            "animation" => {
                let values = properties(declaration);
                animations.push(SceneAnimation {
                    id: string_field(declaration, "id").unwrap_or("").to_string(),
                    reference: values.get("reference").map(|value| unquote(value)),
                    properties: values,
                });
            }
            unknown => diagnostics.push(runtime_diagnostic(
                "warning", "R101", format!("ignored unknown declaration type `{unknown}`"), &ir.source, None,
            )),
        }
    }

    let active_ids = nodes.iter().filter(|node| node.active).map(|node| node.id.as_str())
        .chain(references.iter().map(|reference| reference.id.as_str()))
        .collect::<HashSet<_>>();
    for relation in &mut relations {
        relation.active = active_ids.contains(relation.from.as_str()) && active_ids.contains(relation.to.as_str());
        if !relation.active {
            diagnostics.push(runtime_diagnostic(
                "error", "R210", format!("relation `{} -> {}` has an inactive or missing endpoint", relation.from, relation.to),
                &ir.source, Some(format!("{} -> {}", relation.from, relation.to)),
            ));
        }
    }

    if options.resolve_references {
        let base_dir = input_path.and_then(Path::parent).unwrap_or_else(|| Path::new("."));
        let local_targets = collect_local_targets(&ir.declarations);
        let mut stack = HashSet::new();
        for reference in &mut references {
            resolve_reference(reference, base_dir, &local_targets, options, 0, &mut stack, &mut diagnostics);
        }
    }

    Ok(Scene {
        format: "SRNG-SCENE".to_string(),
        version: ir.version,
        source: ir.source,
        file_id: ir.file_id,
        viewport: Viewport { width: options.viewport_width, height: options.viewport_height, dpi: options.dpi },
        nodes,
        relations,
        references,
        animations,
        diagnostics,
    })
}

fn validate_options(options: &RuntimeOptions) -> Result<(), RuntimeError> {
    if !options.viewport_width.is_finite() || options.viewport_width <= 0.0 ||
       !options.viewport_height.is_finite() || options.viewport_height <= 0.0 ||
       !options.dpi.is_finite() || options.dpi <= 0.0 {
        return Err(RuntimeError::InvalidIr("viewport dimensions and DPI must be finite positive numbers".to_string()));
    }
    Ok(())
}

fn collect_units(declarations: &[Value], source: &str, diagnostics: &mut Vec<RuntimeDiagnostic>) -> HashMap<String, UnitDefinition> {
    let mut units = HashMap::new();
    for declaration in declarations.iter().filter(|value| string_field(value, "type") == Some("unit")) {
        let Some(name) = string_field(declaration, "name") else { continue };
        let Some(scale) = declaration.get("scale").and_then(Value::as_f64) else {
            diagnostics.push(runtime_diagnostic("error", "R120", format!("unit `{name}` has an invalid scale"), source, Some(name.to_string())));
            continue;
        };
        let Some(base) = string_field(declaration, "base") else { continue };
        units.insert(name.to_string(), UnitDefinition { scale, base: base.to_string() });
    }
    units
}

fn build_node(
    declaration: &Value,
    source: &str,
    units: &HashMap<String, UnitDefinition>,
    options: &RuntimeOptions,
    diagnostics: &mut Vec<RuntimeDiagnostic>,
) -> Option<SceneNode> {
    let id = string_field(declaration, "id")?.to_string();
    let kind = string_field(declaration, "kind").unwrap_or("unknown").to_string();
    let values = properties(declaration);
    let mut geometry = Geometry::default();
    let mut active = true;

    match values.get("position") {
        Some(value) => match resolve_pair(value, Axis::Position, units, options) {
            Ok((x, y)) => { geometry.x = Some(x); geometry.y = Some(y); }
            Err(message) => {
                active = false;
                diagnostics.push(runtime_diagnostic("error", "R200", message, source, Some(id.clone())));
            }
        },
        None => {
            active = false;
            diagnostics.push(runtime_diagnostic("error", "R201", format!("node `{id}` cannot run without an explicit position"), source, Some(id.clone())));
        }
    }

    if let Some(value) = values.get("size") {
        match resolve_pair(value, Axis::Size, units, options) {
            Ok((width, height)) if width >= 0.0 && height >= 0.0 => {
                geometry.width = Some(width);
                geometry.height = Some(height);
            }
            Ok(_) => {
                active = false;
                diagnostics.push(runtime_diagnostic("error", "R202", format!("node `{id}` has a negative size"), source, Some(id.clone())));
            }
            Err(message) => {
                active = false;
                diagnostics.push(runtime_diagnostic("error", "R203", message, source, Some(id.clone())));
            }
        }
    }

    Some(SceneNode { id, kind, source: source.to_string(), properties: values, geometry, active })
}

fn build_reference(
    declaration: &Value,
    source: &str,
    units: &HashMap<String, UnitDefinition>,
    options: &RuntimeOptions,
    diagnostics: &mut Vec<RuntimeDiagnostic>,
) -> Option<SceneReference> {
    let id = string_field(declaration, "id")?.to_string();
    let target = string_field(declaration, "target").unwrap_or("");
    let source_file = string_field(declaration, "source_file").unwrap_or_else(|| target.rsplit_once('#').map_or(target, |value| value.0));
    let target_id = string_field(declaration, "target_id").unwrap_or_else(|| target.rsplit_once('#').map_or("", |value| value.1));
    let values = properties(declaration);
    let mut geometry = Geometry::default();
    if let Some(value) = values.get("position") {
        match resolve_pair(value, Axis::Position, units, options) {
            Ok((x, y)) => { geometry.x = Some(x); geometry.y = Some(y); }
            Err(message) => diagnostics.push(runtime_diagnostic("error", "R220", message, source, Some(id.clone()))),
        }
    }
    if let Some(value) = values.get("size") {
        match resolve_pair(value, Axis::Size, units, options) {
            Ok((width, height)) => { geometry.width = Some(width); geometry.height = Some(height); }
            Err(message) => diagnostics.push(runtime_diagnostic("error", "R221", message, source, Some(id.clone()))),
        }
    }
    Some(SceneReference {
        id,
        source_file: source_file.to_string(),
        target_id: target_id.to_string(),
        provenance: target.to_string(),
        resolved: false,
        resolved_kind: None,
        properties: values,
        geometry,
    })
}

#[derive(Clone, Copy)]
enum Axis { Position, Size }

fn resolve_pair(value: &str, axis: Axis, units: &HashMap<String, UnitDefinition>, options: &RuntimeOptions) -> Result<(f64, f64), String> {
    let lengths = parse_lengths(value)?;
    if lengths.len() != 2 {
        return Err(format!("expected two lengths, found {} in `{value}`", lengths.len()));
    }
    let x = resolve_length(lengths[0].0, &lengths[0].1, true, units, options, &mut HashSet::new())?;
    let y = resolve_length(lengths[1].0, &lengths[1].1, false, units, options, &mut HashSet::new())?;
    if matches!(axis, Axis::Size) && (!x.is_finite() || !y.is_finite()) {
        return Err(format!("size is not finite in `{value}`"));
    }
    Ok((x, y))
}

fn parse_lengths(value: &str) -> Result<Vec<(f64, String)>, String> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        if let Ok(number) = parts[index].parse::<f64>() {
            let unit = parts.get(index + 1).filter(|next| next.parse::<f64>().is_err()).copied().unwrap_or("px");
            index += if unit == "px" && parts.get(index + 1).is_none() { 1 } else if parts.get(index + 1).is_some_and(|next| next.parse::<f64>().is_err()) { 2 } else { 1 };
            output.push((number, unit.to_string()));
            continue;
        }
        let split_at = parts[index].char_indices().find(|(_, ch)| !matches!(ch, '0'..='9' | '-' | '+' | '.' | 'e' | 'E')).map(|(position, _)| position);
        let Some(split_at) = split_at else { return Err(format!("invalid length `{}`", parts[index])); };
        let number = parts[index][..split_at].parse::<f64>().map_err(|_| format!("invalid length `{}`", parts[index]))?;
        output.push((number, parts[index][split_at..].to_string()));
        index += 1;
    }
    Ok(output)
}

fn resolve_length(
    value: f64,
    unit: &str,
    horizontal: bool,
    units: &HashMap<String, UnitDefinition>,
    options: &RuntimeOptions,
    visiting: &mut HashSet<String>,
) -> Result<f64, String> {
    let factor = match unit {
        "px" => 1.0,
        "pt" => options.dpi / 72.0,
        "in" => options.dpi,
        "cm" => options.dpi / 2.54,
        "mm" => options.dpi / 25.4,
        "vw" => options.viewport_width / 100.0,
        "vh" => options.viewport_height / 100.0,
        "%" => if horizontal { options.viewport_width / 100.0 } else { options.viewport_height / 100.0 },
        custom => {
            if !visiting.insert(custom.to_string()) {
                return Err(format!("cyclic custom unit `{custom}`"));
            }
            let definition = units.get(custom).ok_or_else(|| format!("unknown unit `{custom}`"))?;
            let base = resolve_length(definition.scale, &definition.base, horizontal, units, options, visiting)?;
            visiting.remove(custom);
            base
        }
    };
    let result = value * factor;
    if result.is_finite() { Ok(result) } else { Err(format!("length `{value}{unit}` is not finite")) }
}

fn resolve_reference(
    reference: &mut SceneReference,
    base_dir: &Path,
    local_targets: &HashMap<String, String>,
    options: &RuntimeOptions,
    depth: usize,
    stack: &mut HashSet<PathBuf>,
    diagnostics: &mut Vec<RuntimeDiagnostic>,
) {
    if reference.target_id.is_empty() {
        diagnostics.push(runtime_diagnostic("error", "R230", format!("reference `{}` has no target id", reference.id), &reference.provenance, Some(reference.id.clone())));
        return;
    }
    if reference.source_file.is_empty() || reference.source_file == "." {
        if let Some(kind) = local_targets.get(&reference.target_id) {
            reference.resolved = true;
            reference.resolved_kind = Some(kind.clone());
        } else {
            diagnostics.push(runtime_diagnostic("error", "R231", format!("reference `{}` cannot find local target `{}`", reference.id, reference.target_id), &reference.provenance, Some(reference.id.clone())));
        }
        return;
    }
    if depth >= options.max_reference_depth {
        diagnostics.push(runtime_diagnostic("error", "R232", format!("reference depth exceeded for `{}`", reference.id), &reference.provenance, Some(reference.id.clone())));
        return;
    }
    if reference.source_file.contains("://") {
        diagnostics.push(runtime_diagnostic("error", "R233", "network references are not supported by the local runtime".to_string(), &reference.provenance, Some(reference.id.clone())));
        return;
    }

    let path = base_dir.join(&reference.source_file);
    let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if !stack.insert(identity.clone()) {
        diagnostics.push(runtime_diagnostic("error", "R234", format!("cyclic reference through `{}`", path.display()), &reference.provenance, Some(reference.id.clone())));
        return;
    }
    let result = load_ir_document(&path).and_then(|document| {
        let targets = collect_local_targets(&document.declarations);
        targets.get(&reference.target_id).cloned().ok_or_else(|| RuntimeError::InvalidIr(format!("target `{}` does not exist in `{}`", reference.target_id, path.display())))
    });
    stack.remove(&identity);
    match result {
        Ok(kind) => {
            reference.resolved = true;
            reference.resolved_kind = Some(kind);
        }
        Err(error) => diagnostics.push(runtime_diagnostic("error", "R235", error.to_string(), &reference.provenance, Some(reference.id.clone()))),
    }
}

fn load_ir_document(path: &Path) -> Result<IrDocument, RuntimeError> {
    let text = fs::read_to_string(path).map_err(|source| RuntimeError::Io { path: path.to_path_buf(), source })?;
    let json = if path.extension().and_then(|value| value.to_str()) == Some("srng") {
        crate::compile_to_json(&text, &path.to_string_lossy())
    } else {
        text
    };
    serde_json::from_str(&json).map_err(|error| RuntimeError::InvalidIr(error.to_string()))
}

fn collect_local_targets(declarations: &[Value]) -> HashMap<String, String> {
    declarations.iter().filter_map(|value| {
        match string_field(value, "type")? {
            "node" => Some((string_field(value, "id")?.to_string(), string_field(value, "kind").unwrap_or("unknown").to_string())),
            "reference" => Some((string_field(value, "id")?.to_string(), "reference".to_string())),
            _ => None,
        }
    }).collect()
}

fn properties(value: &Value) -> BTreeMap<String, String> {
    value.get("properties").and_then(Value::as_object).map(|object| {
        object.iter().filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string()))).collect()
    }).unwrap_or_default()
}

fn string_field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn unquote(value: &str) -> String {
    value.strip_prefix('"').and_then(|value| value.strip_suffix('"')).unwrap_or(value).to_string()
}

fn runtime_diagnostic(
    severity: &str,
    code: &str,
    message: String,
    source: &str,
    declaration: Option<String>,
) -> RuntimeDiagnostic {
    RuntimeDiagnostic {
        severity: severity.to_string(),
        code: code.to_string(),
        message,
        source: source.to_string(),
        declaration,
        line: 0,
        column: 0,
    }
}
