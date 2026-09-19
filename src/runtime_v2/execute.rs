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

fn execute_json_from(ir_json: &str, input_path: Option<&Path>, options: &RuntimeOptions) -> Result<Scene, RuntimeError> {
    validate_options(options)?;
    let ir: IrDocument = serde_json::from_str(ir_json).map_err(|e| RuntimeError::InvalidIr(e.to_string()))?;
    if ir.format != "SRNG-IR" { return Err(RuntimeError::InvalidIr(format!("expected format `SRNG-IR`, found `{}`", ir.format))); }
    let mut diagnostics = ir.diagnostics.iter().map(|d| RuntimeDiagnostic { severity:d.severity.clone(), code:d.code.clone(), message:d.message.clone(), source:ir.source.clone(), declaration:None, line:d.line, column:d.column }).collect::<Vec<_>>();
    let units = collect_units(&ir.declarations, &ir.source, &mut diagnostics);
    let unit_context = units.iter().map(|(name,u)|(name.clone(),UnitContext{scale:u.scale,base:u.base.clone()})).collect();
    let mut nodes=Vec::new(); let mut relations=Vec::new(); let mut references=Vec::new(); let mut animations=Vec::new(); let mut paint_order=0usize;
    for declaration in &ir.declarations {
        match string_field(declaration,"type").unwrap_or("") {
            "unit"=>{},
            "node"=>{ if let Some(node)=build_node(declaration,&ir.source,&units,options,paint_order,&mut diagnostics){nodes.push(node);paint_order+=1;} },
            "relation"=>{let props=properties(declaration);relations.push(SceneRelation{from:string_field(declaration,"from").unwrap_or("").to_string(),to:string_field(declaration,"to").unwrap_or("").to_string(),kind:props.get("kind").map(|v|unquote(v)),properties:props,active:false});},
            "reference"=>{if let Some(reference)=build_reference(declaration,&ir.source,&units,options,paint_order,&mut diagnostics){references.push(reference);paint_order+=1;}},
            "animation"=>{let props=properties(declaration);animations.push(SceneAnimation{id:string_field(declaration,"id").unwrap_or("").to_string(),reference:props.get("reference").map(|v|unquote(v)),properties:props});},
            unknown if !unknown.is_empty()=>diagnostics.push(runtime_diagnostic("warning","R101",format!("ignored unknown declaration type `{unknown}`"),&ir.source,None)),
            _=>{}
        }
    }
    if options.resolve_references {
        let root_path=input_path.map(normalized_path); let base_dir=input_path.and_then(Path::parent).unwrap_or_else(||Path::new(".")); let mut stack=Vec::<RefKey>::new();
        for reference in &mut references {
            match resolve_reference_target(reference,&ir,root_path.as_deref(),base_dir,options,0,&mut stack) {
                Ok(target)=>{reference.resolved=true;reference.resolved_kind=Some(target.kind);reference.linked_geometry=Some(target.geometry);reference.linked_properties=target.properties;reference.resolved_nodes=target.nodes;},
                Err(error)=>{reference.resolved=false;reference.active=false;reference.resolved_nodes.clear();diagnostics.push(runtime_diagnostic("error",error.code,error.message,&reference.provenance,Some(reference.id.clone())));}
            }
        }
        let mut instance_nodes=Vec::new();
        for reference in references.iter_mut().filter(|r|r.active&&r.resolved&&!r.resolved_nodes.is_empty()) {
            let mut children=reference.resolved_nodes.clone(); children.sort_by_key(|node|node.paint_order);
            for child in children {
                if child.kind=="group" {continue;}
                let mut props=child.properties.clone(); props.remove("resource-only"); props.insert("resource-instance".into(),reference.id.clone()); props.insert("resource-source-id".into(),child.source_id.clone()); props.insert("resource-provenance".into(),reference.provenance.clone()); apply_instance_style(&mut props,&reference.properties);
                let mut geometry=instantiate_geometry(&child.geometry,&reference.geometry,&reference.linked_properties,&reference.properties); apply_instance_transform(&mut props,&reference.properties,&mut geometry);
                instance_nodes.push(SceneNode{id:format!("{}::{}",reference.id,child.source_id),kind:child.kind,source:child.source,properties:props,geometry,paint_order:reference.paint_order.saturating_add(child.paint_order),active:true});
            }
            reference.active=false;
        }
        nodes.extend(instance_nodes);
    } else { for reference in &mut references {reference.active=false;} }
    let active_ids=nodes.iter().filter(|n|n.active).map(|n|n.id.as_str()).chain(references.iter().filter(|r|r.active).map(|r|r.id.as_str())).collect::<HashSet<_>>();
    for relation in &mut relations {
        let resource_relation=relation.properties.get("kind").map(|v|unquote(v)).as_deref()==Some("contains")&&nodes.iter().any(|node|node.id==relation.from&&node.properties.get("resource-only").is_some());
        relation.active=!resource_relation&&active_ids.contains(relation.from.as_str())&&active_ids.contains(relation.to.as_str());
        if !relation.active&&!resource_relation {diagnostics.push(runtime_diagnostic("error","R210",format!("relation `{} -> {}` has an inactive or missing endpoint",relation.from,relation.to),&ir.source,Some(format!("{} -> {}",relation.from,relation.to))));}
    }
    Ok(Scene{format:"SRNG-SCENE".into(),version:ir.version,source:ir.source,file_id:ir.file_id,viewport:Viewport{width:options.viewport_width,height:options.viewport_height,dpi:options.dpi},unit_context,nodes,relations,references,animations,diagnostics})
}

fn apply_instance_style(properties:&mut BTreeMap<String,String>,instance:&BTreeMap<String,String>){
    for key in ["fill","stroke"] { let inherits=properties.get(&format!("resource-inherit-{key}")).is_some_and(|v|unquote(v)=="true"); if inherits { if let Some(value)=instance.get(key){properties.insert(key.to_string(),value.clone());} else if key=="fill" {properties.insert(key.to_string(),"#000000".into());} else {properties.insert(key.to_string(),"none".into());} } }
    for key in ["opacity","fill-opacity","stroke-opacity"] {if let Some(value)=instance.get(key){properties.insert(key.to_string(),value.clone());}}
}

fn instantiate_geometry(source:&Geometry,instance:&Geometry,target_properties:&BTreeMap<String,String>,instance_properties:&BTreeMap<String,String>)->Geometry{
    let dx=instance.x.unwrap_or(0.0); let dy=instance.y.unwrap_or(0.0); let mut scale_x=1.0; let mut scale_y=1.0; let mut offset_x=dx; let mut offset_y=dy;
    if target_properties.get("resource-kind").map(|v|unquote(v)).as_deref()==Some("symbol") {
        if let (Some(viewbox),Some(width),Some(height))=(target_properties.get("viewbox").and_then(|v|parse_viewbox_runtime(v)),instance.width,instance.height){let preserve=instance_properties.get("preserve-aspect-ratio").or_else(||target_properties.get("preserve-aspect-ratio")).map(|v|unquote(v)).unwrap_or_else(||"xMidYMid meet".into());let mapped=symbol_mapping(viewbox,width,height,&preserve);scale_x=mapped.0;scale_y=mapped.1;offset_x+=mapped.2;offset_y+=mapped.3;}
    }
    Geometry{x:source.x.map(|v|offset_x+v*scale_x),y:source.y.map(|v|offset_y+v*scale_y),width:source.width.map(|v|v*scale_x.abs()),height:source.height.map(|v|v*scale_y.abs())}
}
fn parse_viewbox_runtime(value:&str)->Option<(f64,f64,f64,f64)>{let value=unquote(value);let values=value.split(|ch:char|ch==','||ch.is_whitespace()).filter(|v|!v.is_empty()).map(str::parse::<f64>).collect::<Result<Vec<_>,_>>().ok()?;if values.len()!=4||values[2]<=0.0||values[3]<=0.0{return None;}Some((values[0],values[1],values[2],values[3]))}
fn symbol_mapping(viewbox:(f64,f64,f64,f64),width:f64,height:f64,preserve:&str)->(f64,f64,f64,f64){let(min_x,min_y,vb_width,vb_height)=viewbox;let sx=width/vb_width;let sy=height/vb_height;if preserve.trim()=="none"{return(sx,sy,-min_x*sx,-min_y*sy);}let meet=!preserve.split_whitespace().any(|t|t=="slice");let scale=if meet{sx.min(sy)}else{sx.max(sy)};let rw=vb_width*scale;let rh=vb_height*scale;let align=preserve.split_whitespace().next().unwrap_or("xMidYMid");let ex=if align.contains("xMax"){width-rw}else if align.contains("xMid"){(width-rw)/2.0}else{0.0};let ey=if align.contains("YMax"){height-rh}else if align.contains("YMid"){(height-rh)/2.0}else{0.0};(scale,scale,ex-min_x*scale,ey-min_y*scale)}
fn apply_instance_transform(properties:&mut BTreeMap<String,String>,instance:&BTreeMap<String,String>,geometry:&mut Geometry){let Some(transform)=instance.get("transform")else{return;};let transform=unquote(transform);properties.insert("transform".into(),format!("\"{}\"",transform));if let Some((tx,ty))=parse_translate(&transform){geometry.x=geometry.x.map(|v|v+tx);geometry.y=geometry.y.map(|v|v+ty);}}
fn parse_translate(transform:&str)->Option<(f64,f64)>{let inner=transform.trim().strip_prefix("translate(")?.strip_suffix(')')?;let values=inner.split(|ch:char|ch==','||ch.is_whitespace()).filter(|v|!v.is_empty()).map(str::parse::<f64>).collect::<Result<Vec<_>,_>>().ok()?;match values.as_slice(){[x]=>Some((*x,0.0)),[x,y]=>Some((*x,*y)),_=>None}}
