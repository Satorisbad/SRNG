pub fn execute_json(ir_json: &str, options: &RuntimeOptions) -> Result<Scene, RuntimeError> {
    execute_json_from(ir_json, None, options)
}

pub fn execute_file(path: impl AsRef<Path>, options: &RuntimeOptions) -> Result<Scene, RuntimeError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let ir_json = if path.extension().and_then(|x| x.to_str()) == Some("srng") {
        crate::compile_to_json(&text, &path.to_string_lossy())
    } else {
        text
    };
    execute_json_from(&ir_json, Some(path), options)
}

fn execute_json_from(
    ir_json: &str,
    input_path: Option<&Path>,
    options: &RuntimeOptions,
) -> Result<Scene, RuntimeError> {
    validate_options(options)?;
    let ir: IrDocument = serde_json::from_str(ir_json)
        .map_err(|e| RuntimeError::InvalidIr(e.to_string()))?;
    if ir.format != "SRNG-IR" {
        return Err(RuntimeError::InvalidIr(format!(
            "expected format `SRNG-IR`, found `{}`",
            ir.format
        )));
    }

    let mut diagnostics = ir
        .diagnostics
        .iter()
        .map(|d| RuntimeDiagnostic {
            severity: d.severity.clone(),
            code: d.code.clone(),
            message: d.message.clone(),
            source: ir.source.clone(),
            declaration: None,
            line: d.line,
            column: d.column,
        })
        .collect::<Vec<_>>();

    let units = collect_units(&ir.declarations, &ir.source, &mut diagnostics);
    let unit_context = units
        .iter()
        .map(|(name, u)| {
            (
                name.clone(),
                UnitContext {
                    scale: u.scale,
                    base: u.base.clone(),
                },
            )
        })
        .collect();

    let mut nodes = Vec::new();
    let mut relations = Vec::new();
    let mut references = Vec::new();
    let mut animations = Vec::new();
    let mut paint_order = 0usize;

    for declaration in &ir.declarations {
        match string_field(declaration, "type").unwrap_or("") {
            "unit" => {}
            "node" => {
                if let Some(node) = build_node(
                    declaration,
                    &ir.source,
                    &units,
                    options,
                    paint_order,
                    &mut diagnostics,
                ) {
                    nodes.push(node);
                    paint_order += 1;
                }
            }
            "relation" => {
                let props = properties(declaration);
                relations.push(SceneRelation {
                    from: string_field(declaration, "from").unwrap_or("").to_string(),
                    to: string_field(declaration, "to").unwrap_or("").to_string(),
                    kind: props.get("kind").map(|v| unquote(v)),
                    properties: props,
                    active: false,
                });
            }
            "reference" => {
                if let Some(reference) = build_reference(
                    declaration,
                    &ir.source,
                    &units,
                    options,
                    paint_order,
                    &mut diagnostics,
                ) {
                    references.push(reference);
                    paint_order += 1;
                }
            }
            "animation" => {
                let props = properties(declaration);
                animations.push(SceneAnimation {
                    id: string_field(declaration, "id").unwrap_or("").to_string(),
                    reference: props.get("reference").map(|v| unquote(v)),
                    properties: props,
                });
            }
            unknown if !unknown.is_empty() => diagnostics.push(runtime_diagnostic(
                "warning",
                "R101",
                format!("ignored unknown declaration type `{unknown}`"),
                &ir.source,
                None,
            )),
            _ => {}
        }
    }

    if options.resolve_references {
        let root_path = input_path.map(normalized_path);
        let base_dir = input_path
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let mut stack = Vec::<RefKey>::new();
        for reference in &mut references {
            match resolve_reference_target(
                reference,
                &ir,
                root_path.as_deref(),
                base_dir,
                options,
                0,
                &mut stack,
            ) {
                Ok(target) => {
                    reference.resolved = true;
                    reference.resolved_kind = Some(target.kind);
                    reference.linked_geometry = Some(target.geometry);
                    reference.linked_properties = target.properties;
                }
                Err(error) => {
                    reference.resolved = false;
                    reference.active = false;
                    diagnostics.push(runtime_diagnostic(
                        "error",
                        error.code,
                        error.message,
                        &reference.provenance,
                        Some(reference.id.clone()),
                    ));
                }
            }
        }
    } else {
        for reference in &mut references {
            reference.active = false;
        }
    }

    let active_ids = nodes
        .iter()
        .filter(|n| n.active)
        .map(|n| n.id.as_str())
        .chain(
            references
                .iter()
                .filter(|r| r.active)
                .map(|r| r.id.as_str()),
        )
        .collect::<HashSet<_>>();
    for relation in &mut relations {
        relation.active = active_ids.contains(relation.from.as_str())
            && active_ids.contains(relation.to.as_str());
        if !relation.active {
            diagnostics.push(runtime_diagnostic(
                "error",
                "R210",
                format!(
                    "relation `{} -> {}` has an inactive or missing endpoint",
                    relation.from, relation.to
                ),
                &ir.source,
                Some(format!("{} -> {}", relation.from, relation.to)),
            ));
        }
    }

    Ok(Scene {
        format: "SRNG-SCENE".to_string(),
        version: ir.version,
        source: ir.source,
        file_id: ir.file_id,
        viewport: Viewport {
            width: options.viewport_width,
            height: options.viewport_height,
            dpi: options.dpi,
        },
        unit_context,
        nodes,
        relations,
        references,
        animations,
        diagnostics,
    })
}
