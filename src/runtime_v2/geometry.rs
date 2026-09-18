fn validate_options(options: &RuntimeOptions) -> Result<(), RuntimeError> {
    if !options.viewport_width.is_finite()
        || options.viewport_width <= 0.0
        || !options.viewport_height.is_finite()
        || options.viewport_height <= 0.0
        || !options.dpi.is_finite()
        || options.dpi <= 0.0
        || options.max_reference_depth == 0
    {
        return Err(RuntimeError::InvalidIr(
            "viewport dimensions, DPI, and reference depth must be positive".to_string(),
        ));
    }
    Ok(())
}

fn collect_units(
    declarations: &[Value],
    source: &str,
    diagnostics: &mut Vec<RuntimeDiagnostic>,
) -> HashMap<String, UnitDefinition> {
    let mut units = HashMap::new();
    for declaration in declarations
        .iter()
        .filter(|v| string_field(v, "type") == Some("unit"))
    {
        let Some(name) = string_field(declaration, "name") else {
            continue;
        };
        let Some(scale) = declaration.get("scale").and_then(Value::as_f64) else {
            diagnostics.push(runtime_diagnostic(
                "error",
                "R120",
                format!("unit `{name}` has an invalid scale"),
                source,
                Some(name.to_string()),
            ));
            continue;
        };
        if !scale.is_finite() {
            diagnostics.push(runtime_diagnostic(
                "error",
                "R120",
                format!("unit `{name}` has a non-finite scale"),
                source,
                Some(name.to_string()),
            ));
            continue;
        }
        let Some(base) = string_field(declaration, "base") else {
            continue;
        };
        units.insert(
            name.to_string(),
            UnitDefinition {
                scale,
                base: base.to_string(),
            },
        );
    }
    units
}

fn build_node(
    declaration: &Value,
    source: &str,
    units: &HashMap<String, UnitDefinition>,
    options: &RuntimeOptions,
    paint_order: usize,
    diagnostics: &mut Vec<RuntimeDiagnostic>,
) -> Option<SceneNode> {
    let id = string_field(declaration, "id")?.to_string();
    let kind = string_field(declaration, "kind").unwrap_or("unknown").to_string();
    let props = properties(declaration);
    let resource_only = props.get("resource-only").is_some_and(|value| unquote(value) == "true");
    let mut geometry = Geometry::default();
    let mut active = true;

    match props.get("position") {
        Some(value) => match resolve_pair(value, units, options) {
            Ok((x, y)) => {
                geometry.x = Some(x);
                geometry.y = Some(y);
            }
            Err(message) => {
                active = false;
                diagnostics.push(runtime_diagnostic(
                    "error",
                    "R200",
                    message,
                    source,
                    Some(id.clone()),
                ));
            }
        },
        None => {
            active = false;
            diagnostics.push(runtime_diagnostic(
                "error",
                "R201",
                format!("node `{id}` cannot run without an explicit position"),
                source,
                Some(id.clone()),
            ));
        }
    }

    if let Some(value) = props.get("size") {
        match resolve_pair(value, units, options) {
            Ok((width, height)) if width >= 0.0 && height >= 0.0 => {
                geometry.width = Some(width);
                geometry.height = Some(height);
            }
            Ok(_) => {
                active = false;
                diagnostics.push(runtime_diagnostic(
                    "error",
                    "R202",
                    format!("node `{id}` has a negative size"),
                    source,
                    Some(id.clone()),
                ));
            }
            Err(message) => {
                active = false;
                diagnostics.push(runtime_diagnostic(
                    "error",
                    "R203",
                    message,
                    source,
                    Some(id.clone()),
                ));
            }
        }
    }

    if resource_only && active {
        active = false;
    }

    Some(SceneNode {
        id,
        kind,
        source: source.to_string(),
        properties: props,
        geometry,
        paint_order,
        active,
    })
}

fn build_reference(
    declaration: &Value,
    source: &str,
    units: &HashMap<String, UnitDefinition>,
    options: &RuntimeOptions,
    paint_order: usize,
    diagnostics: &mut Vec<RuntimeDiagnostic>,
) -> Option<SceneReference> {
    let id = string_field(declaration, "id")?.to_string();
    let target = string_field(declaration, "target").unwrap_or("");
    let (fallback_source, fallback_id) = split_target(target);
    let source_file = string_field(declaration, "source_file").unwrap_or(fallback_source);
    let target_id = string_field(declaration, "target_id").unwrap_or(fallback_id);
    let props = properties(declaration);
    let resource_only = props.get("resource-only").is_some_and(|value| unquote(value) == "true");
    let mut geometry = Geometry::default();
    let mut geometry_valid = true;

    if let Some(value) = props.get("position") {
        match resolve_pair(value, units, options) {
            Ok((x, y)) => {
                geometry.x = Some(x);
                geometry.y = Some(y);
            }
            Err(message) => {
                geometry_valid = false;
                diagnostics.push(runtime_diagnostic(
                    "error",
                    "R220",
                    message,
                    source,
                    Some(id.clone()),
                ));
            }
        }
    }
    if let Some(value) = props.get("size") {
        match resolve_pair(value, units, options) {
            Ok((width, height)) if width >= 0.0 && height >= 0.0 => {
                geometry.width = Some(width);
                geometry.height = Some(height);
            }
            Ok(_) => {
                geometry_valid = false;
                diagnostics.push(runtime_diagnostic(
                    "error",
                    "R221",
                    format!("reference `{id}` has a negative size"),
                    source,
                    Some(id.clone()),
                ));
            }
            Err(message) => {
                geometry_valid = false;
                diagnostics.push(runtime_diagnostic(
                    "error",
                    "R221",
                    message,
                    source,
                    Some(id.clone()),
                ));
            }
        }
    }

    Some(SceneReference {
        id,
        source_file: source_file.to_string(),
        target_id: target_id.to_string(),
        provenance: target.to_string(),
        resolved: false,
        active: geometry_valid && !resource_only,
        resolved_kind: None,
        properties: props,
        geometry,
        linked_geometry: None,
        linked_properties: BTreeMap::new(),
        resolved_nodes: Vec::new(),
        paint_order,
    })
}

fn resolve_pair(
    value: &str,
    units: &HashMap<String, UnitDefinition>,
    options: &RuntimeOptions,
) -> Result<(f64, f64), String> {
    let lengths = parse_lengths(value)?;
    if lengths.len() != 2 {
        return Err(format!(
            "expected two lengths, found {} in `{value}`",
            lengths.len()
        ));
    }
    let x = resolve_length(
        lengths[0].0,
        &lengths[0].1,
        true,
        units,
        options,
        &mut HashSet::new(),
    )?;
    let y = resolve_length(
        lengths[1].0,
        &lengths[1].1,
        false,
        units,
        options,
        &mut HashSet::new(),
    )?;
    Ok((x, y))
}

fn parse_lengths(value: &str) -> Result<Vec<(f64, String)>, String> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        let part = parts[index];
        if let Ok(number) = part.parse::<f64>() {
            let has_explicit_unit = parts
                .get(index + 1)
                .is_some_and(|next| next.parse::<f64>().is_err());
            let unit = if has_explicit_unit {
                parts[index + 1]
            } else {
                "px"
            };
            index += if has_explicit_unit { 2 } else { 1 };
            output.push((number, unit.to_string()));
            continue;
        }

        let split_at = numeric_prefix_len(part)
            .ok_or_else(|| format!("invalid length `{part}`"))?;
        if split_at == 0 || split_at == part.len() {
            return Err(format!("invalid length `{part}`"));
        }
        let number = part[..split_at]
            .parse::<f64>()
            .map_err(|_| format!("invalid length `{part}`"))?;
        output.push((number, part[split_at..].to_string()));
        index += 1;
    }
    Ok(output)
}

fn numeric_prefix_len(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut i = 0usize;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        i += 1;
    }
    let mut digits = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        let exp_start = i;
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        let before = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == before {
            i = exp_start;
        }
    }
    Some(i)
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
        "%" => {
            if horizontal {
                options.viewport_width / 100.0
            } else {
                options.viewport_height / 100.0
            }
        }
        custom => {
            if !visiting.insert(custom.to_string()) {
                return Err(format!("cyclic custom unit `{custom}`"));
            }
            let definition = units
                .get(custom)
                .ok_or_else(|| format!("unknown unit `{custom}`"))?;
            let base = resolve_length(
                definition.scale,
                &definition.base,
                horizontal,
                units,
                options,
                visiting,
            )?;
            visiting.remove(custom);
            base
        }
    };
    let result = value * factor;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(format!("length `{value}{unit}` is not finite"))
    }
}