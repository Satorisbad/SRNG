#[derive(Debug)]
struct ResolveFailure {
    code: &'static str,
    message: String,
}

fn resolve_reference_target(
    reference: &SceneReference,
    current_ir: &IrDocument,
    current_path: Option<&Path>,
    base_dir: &Path,
    options: &RuntimeOptions,
    depth: usize,
    stack: &mut Vec<RefKey>,
) -> Result<ResolvedTarget, ResolveFailure> {
    if reference.target_id.is_empty() {
        return Err(failure("R230", format!("reference `{}` has no target id", reference.id)));
    }
    if depth >= options.max_reference_depth {
        return Err(failure("R232", format!("reference depth exceeded for `{}`", reference.id)));
    }
    if reference.source_file.contains("://") {
        return Err(failure(
            "R233",
            "network references are not supported by the local runtime".to_string(),
        ));
    }

    let local = reference.source_file.is_empty() || reference.source_file == ".";
    if local {
        let path = current_path
            .map(normalized_path)
            .unwrap_or_else(|| PathBuf::from("<memory>"));
        return resolve_target_in_document(
            current_ir,
            current_path,
            base_dir,
            &reference.target_id,
            path,
            options,
            depth + 1,
            stack,
        );
    }

    let path = normalized_path(&base_dir.join(&reference.source_file));
    let document = load_ir_document(&path).map_err(|error| failure("R235", error.to_string()))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    resolve_target_in_document(
        &document,
        Some(&path),
        parent,
        &reference.target_id,
        path.clone(),
        options,
        depth + 1,
        stack,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_target_in_document(
    document: &IrDocument,
    document_path: Option<&Path>,
    base_dir: &Path,
    target_id: &str,
    identity_path: PathBuf,
    options: &RuntimeOptions,
    depth: usize,
    stack: &mut Vec<RefKey>,
) -> Result<ResolvedTarget, ResolveFailure> {
    if depth > options.max_reference_depth {
        return Err(failure(
            "R232",
            format!("reference depth exceeded while resolving `{target_id}`"),
        ));
    }

    let key = RefKey {
        path: identity_path.clone(),
        id: target_id.to_string(),
    };
    if stack.contains(&key) {
        let mut chain = stack
            .iter()
            .map(|k| format!("{}#{}", k.path.display(), k.id))
            .collect::<Vec<_>>();
        chain.push(format!("{}#{}", key.path.display(), key.id));
        return Err(failure(
            "R234",
            format!("cyclic reference: {}", chain.join(" -> ")),
        ));
    }
    stack.push(key);

    let result = (|| {
        let declaration = document
            .declarations
            .iter()
            .find(|v| string_field(v, "id") == Some(target_id))
            .ok_or_else(|| {
                failure(
                    "R231",
                    format!("target `{target_id}` does not exist in `{}`", document.source),
                )
            })?;

        match string_field(declaration, "type") {
            Some("node") => {
                let units = collect_units_silent(&document.declarations);
                let mut sink = Vec::new();
                let node = build_node(
                    declaration,
                    &document.source,
                    &units,
                    options,
                    0,
                    &mut sink,
                )
                .ok_or_else(|| failure("R235", format!("invalid target `{target_id}`")))?;
                let resource_only = node
                    .properties
                    .get("resource-only")
                    .is_some_and(|value| unquote(value) == "true");
                if !node.active && !resource_only {
                    return Err(failure(
                        "R235",
                        format!("target `{target_id}` has invalid geometry"),
                    ));
                }

                let nodes = collect_resource_subtree(
                    document,
                    document_path,
                    base_dir,
                    target_id,
                    options,
                    depth,
                    stack,
                )?;
                Ok(ResolvedTarget {
                    kind: node.kind,
                    geometry: node.geometry,
                    properties: node.properties,
                    nodes,
                })
            }
            Some("reference") => {
                let units = collect_units_silent(&document.declarations);
                let mut sink = Vec::new();
                let nested = build_reference(
                    declaration,
                    &document.source,
                    &units,
                    options,
                    0,
                    &mut sink,
                )
                .ok_or_else(|| failure("R235", format!("invalid reference `{target_id}`")))?;
                resolve_reference_target(
                    &nested,
                    document,
                    document_path,
                    base_dir,
                    options,
                    depth,
                    stack,
                )
            }
            other => Err(failure(
                "R235",
                format!(
                    "target `{target_id}` in `{}` is not a node or reference (found {:?})",
                    document.source, other
                ),
            )),
        }
    })();

    stack.pop();
    result
}

#[allow(clippy::too_many_arguments)]
fn collect_resource_subtree(
    document: &IrDocument,
    document_path: Option<&Path>,
    base_dir: &Path,
    root_id: &str,
    options: &RuntimeOptions,
    depth: usize,
    stack: &mut Vec<RefKey>,
) -> Result<Vec<ResolvedResourceNode>, ResolveFailure> {
    let mut children = HashMap::<String, Vec<String>>::new();
    for declaration in &document.declarations {
        if string_field(declaration, "type") != Some("relation") {
            continue;
        }
        let props = properties(declaration);
        if props.get("kind").map(|v| unquote(v)).as_deref() != Some("contains") {
            continue;
        }
        let from = string_field(declaration, "from").unwrap_or("");
        let to = string_field(declaration, "to").unwrap_or("");
        children.entry(from.to_string()).or_default().push(to.to_string());
    }

    let mut output = Vec::new();
    let mut order = 0usize;
    collect_resource_children(
        document,
        document_path,
        base_dir,
        root_id,
        root_id,
        &children,
        options,
        depth,
        stack,
        &mut order,
        &mut output,
    )?;
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn collect_resource_children(
    document: &IrDocument,
    document_path: Option<&Path>,
    base_dir: &Path,
    parent_id: &str,
    root_id: &str,
    children: &HashMap<String, Vec<String>>,
    options: &RuntimeOptions,
    depth: usize,
    stack: &mut Vec<RefKey>,
    order: &mut usize,
    output: &mut Vec<ResolvedResourceNode>,
) -> Result<(), ResolveFailure> {
    for child_id in children.get(parent_id).into_iter().flatten() {
        let declaration = document
            .declarations
            .iter()
            .find(|v| string_field(v, "id") == Some(child_id.as_str()))
            .ok_or_else(|| failure("R231", format!("resource child `{child_id}` is missing")))?;
        match string_field(declaration, "type") {
            Some("node") => {
                let units = collect_units_silent(&document.declarations);
                let mut sink = Vec::new();
                let node = build_node(
                    declaration,
                    &document.source,
                    &units,
                    options,
                    *order,
                    &mut sink,
                )
                .ok_or_else(|| failure("R235", format!("invalid resource node `{child_id}`")))?;
                let resource_only = node
                    .properties
                    .get("resource-only")
                    .is_some_and(|value| unquote(value) == "true");
                if node.active || resource_only {
                    output.push(ResolvedResourceNode {
                        id: format!("{root_id}::{child_id}"),
                        source_id: child_id.clone(),
                        kind: node.kind,
                        source: document.source.clone(),
                        properties: node.properties,
                        geometry: node.geometry,
                        parent_source_id: Some(parent_id.to_string()),
                        paint_order: *order,
                    });
                    *order += 1;
                }
                collect_resource_children(
                    document,
                    document_path,
                    base_dir,
                    child_id,
                    root_id,
                    children,
                    options,
                    depth,
                    stack,
                    order,
                    output,
                )?;
            }
            Some("reference") => {
                let units = collect_units_silent(&document.declarations);
                let mut sink = Vec::new();
                let nested = build_reference(
                    declaration,
                    &document.source,
                    &units,
                    options,
                    *order,
                    &mut sink,
                )
                .ok_or_else(|| failure("R235", format!("invalid nested reference `{child_id}`")))?;
                let target = resolve_reference_target(
                    &nested,
                    document,
                    document_path,
                    base_dir,
                    options,
                    depth,
                    stack,
                )?;
                if target.nodes.is_empty() {
                    output.push(ResolvedResourceNode {
                        id: format!("{root_id}::{child_id}"),
                        source_id: child_id.clone(),
                        kind: target.kind,
                        source: document.source.clone(),
                        properties: merge_resource_properties(&target.properties, &nested.properties),
                        geometry: offset_resource_geometry(&target.geometry, &nested.geometry),
                        parent_source_id: Some(parent_id.to_string()),
                        paint_order: *order,
                    });
                    *order += 1;
                } else {
                    for resolved in target.nodes {
                        let mut resolved = resolved;
                        resolved.id = format!("{root_id}::{child_id}::{}", resolved.source_id);
                        resolved.properties = merge_resource_properties(&resolved.properties, &nested.properties);
                        resolved.geometry = offset_resource_geometry(&resolved.geometry, &nested.geometry);
                        resolved.parent_source_id = Some(parent_id.to_string());
                        resolved.paint_order = *order;
                        *order += 1;
                        output.push(resolved);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn offset_resource_geometry(linked: &Geometry, authored: &Geometry) -> Geometry {
    Geometry {
        x: match (linked.x, authored.x) { (Some(x), Some(dx)) => Some(x + dx), (x, None) => x, (None, x) => x },
        y: match (linked.y, authored.y) { (Some(y), Some(dy)) => Some(y + dy), (y, None) => y, (None, y) => y },
        width: authored.width.or(linked.width),
        height: authored.height.or(linked.height),
    }
}

fn merge_resource_properties(
    linked: &BTreeMap<String, String>,
    authored: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut merged = linked.clone();
    for (key, value) in authored {
        if matches!(key.as_str(), "fill" | "stroke" | "opacity" | "fill-opacity" | "stroke-opacity" | "transform" | "preserve-aspect-ratio" | "viewbox") {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn collect_units_silent(declarations: &[Value]) -> HashMap<String, UnitDefinition> {
    declarations
        .iter()
        .filter(|v| string_field(v, "type") == Some("unit"))
        .filter_map(|v| {
            Some((
                string_field(v, "name")?.to_string(),
                UnitDefinition {
                    scale: v.get("scale")?.as_f64()?,
                    base: string_field(v, "base")?.to_string(),
                },
            ))
        })
        .collect()
}

fn load_ir_document(path: &Path) -> Result<IrDocument, RuntimeError> {
    let text = fs::read_to_string(path).map_err(|source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let json = if path.extension().and_then(|x| x.to_str()) == Some("srng") {
        crate::compile_to_json(&text, &path.to_string_lossy())
    } else {
        text
    };
    let ir: IrDocument = serde_json::from_str(&json)
        .map_err(|e| RuntimeError::InvalidIr(e.to_string()))?;
    if ir.format != "SRNG-IR" {
        return Err(RuntimeError::InvalidIr(format!(
            "expected format `SRNG-IR`, found `{}`",
            ir.format
        )));
    }
    Ok(ir)
}

fn normalized_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}

fn failure(code: &'static str, message: String) -> ResolveFailure {
    ResolveFailure { code, message }
}

fn properties(value: &Value) -> BTreeMap<String, String> {
    value
        .get("properties")
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn string_field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn split_target(target: &str) -> (&str, &str) {
    target.rsplit_once('#').unwrap_or((target, ""))
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
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