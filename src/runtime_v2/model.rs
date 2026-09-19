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
    #[serde(default)]
    pub unit_context: BTreeMap<String, UnitContext>,
    pub nodes: Vec<SceneNode>,
    pub relations: Vec<SceneRelation>,
    pub references: Vec<SceneReference>,
    pub animations: Vec<SceneAnimation>,
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

impl Scene {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == "error")
    }

    pub fn to_json_pretty(&self) -> Result<String, RuntimeError> {
        serde_json::to_string_pretty(self).map_err(|e| RuntimeError::InvalidIr(e.to_string()))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    pub dpi: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnitContext {
    pub scale: f64,
    pub base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNode {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub properties: BTreeMap<String, String>,
    pub geometry: Geometry,
    pub paint_order: usize,
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
pub struct ResolvedResourceNode {
    pub id: String,
    pub source_id: String,
    pub kind: String,
    pub source: String,
    pub properties: BTreeMap<String, String>,
    pub geometry: Geometry,
    pub parent_source_id: Option<String>,
    pub paint_order: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneReference {
    pub id: String,
    pub source_file: String,
    pub target_id: String,
    pub provenance: String,
    pub resolved: bool,
    pub active: bool,
    pub resolved_kind: Option<String>,
    pub properties: BTreeMap<String, String>,
    pub geometry: Geometry,
    pub linked_geometry: Option<Geometry>,
    #[serde(default)]
    pub linked_properties: BTreeMap<String, String>,
    #[serde(default)]
    pub resolved_nodes: Vec<ResolvedResourceNode>,
    pub paint_order: usize,
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

#[derive(Debug, Clone)]
struct ResolvedTarget {
    kind: String,
    geometry: Geometry,
    properties: BTreeMap<String, String>,
    nodes: Vec<ResolvedResourceNode>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RefKey {
    path: PathBuf,
    id: String,
}