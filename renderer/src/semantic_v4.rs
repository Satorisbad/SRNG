use crate::{PreparedScene, RevisionGate};
use kurbo::{Affine, BezPath};
use srng::runtime::{Geometry, Scene, SceneNode};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64, b: f64, c: f64, d: f64, e: f64, f: f64,
}

impl Matrix {
    const ID: Self = Self { a:1.0,b:0.0,c:0.0,d:1.0,e:0.0,f:0.0 };
    fn mul(self, rhs: Self) -> Self {
        Self {
            a:self.a*rhs.a+self.c*rhs.b,
            b:self.b*rhs.a+self.d*rhs.b,
            c:self.a*rhs.c+self.c*rhs.d,
            d:self.b*rhs.c+self.d*rhs.d,
            e:self.a*rhs.e+self.c*rhs.f+self.e,
            f:self.b*rhs.e+self.d*rhs.f+self.f,
        }
    }
    fn translate(x:f64,y:f64)->Self{ Self{e:x,f:y,..Self::ID} }
    fn scale(x:f64,y:f64)->Self{ Self{a:x,d:y,..Self::ID} }
    fn rotate(rad:f64)->Self{ let (s,c)=rad.sin_cos(); Self{a:c,b:s,c:-s,d:c,e:0.0,f:0.0} }
    fn skew(x:f64,y:f64)->Self{ Self{a:1.0,b:y.tan(),c:x.tan(),d:1.0,e:0.0,f:0.0} }
    fn point(self,x:f64,y:f64)->(f64,f64){ (self.a*x+self.c*y+self.e,self.b*x+self.d*y+self.f) }
    fn affine(self)->Affine{ Affine::new([self.a,self.b,self.c,self.d,self.e,self.f]) }
    fn svg(self)->String{ format!("matrix({} {} {} {} {} {})",fmt(self.a),fmt(self.b),fmt(self.c),fmt(self.d),fmt(self.e),fmt(self.f)) }
}

pub fn prepare_scene(scene:&Scene, revision:u64, gate:&RevisionGate)->PreparedScene {
    let mut normalized=scene.clone();
    let parent=parent_map(&normalized);
    let root_view=viewbox_roots(&normalized,&parent);
    let mut memo=HashMap::new();
    let mut visiting=HashSet::new();
    let ids=normalized.nodes.iter().map(|n|n.id.clone()).collect::<Vec<_>>();
    let props=normalized.nodes.iter().map(|n|(n.id.clone(),n.properties.clone())).collect::<HashMap<_,_>>();
    let mut world=HashMap::new();
    for id in &ids {
        let m=world_matrix(id,&parent,&props,&root_view,&mut memo,&mut visiting);
        world.insert(id.clone(),m);
    }

    let clip_ids=normalized.nodes.iter().filter_map(|n| n.properties.get("clip-units").map(|_|n.id.clone())).collect::<HashSet<_>>();

    for node in &mut normalized.nodes {
        let matrix=*world.get(&node.id).unwrap_or(&Matrix::ID);
        preprocess_paint(node, matrix, normalized.viewport.width, normalized.viewport.height);
        preprocess_mask(node, matrix);

        if node.kind=="text" && !node.properties.contains_key("data") {
            synthesize_text(node,matrix,normalized.viewport.width,normalized.viewport.height);
            continue;
        }
        if node.kind=="group" && node.properties.contains_key("use-data") {
            let x=prop_number(&node.properties,"source-x").unwrap_or(0.0);
            let y=prop_number(&node.properties,"source-y").unwrap_or(0.0);
            if let Some(data)=node.properties.get("use-data").map(|v|unquote(v)) {
                node.properties.insert("data".into(),quote(&data));
                if let Some(fill)=node.properties.get("use-fill").cloned(){node.properties.insert("fill".into(),fill);}
                if let Some(stroke)=node.properties.get("use-stroke").cloned(){node.properties.insert("stroke".into(),stroke);}
                bake_node_path(node,matrix.mul(Matrix::translate(x,y)));
            }
            continue;
        }
        if node.kind=="group" {
            if let Some(href)=node.properties.get("href").map(|v|unquote(v)) {
                if href.starts_with("data:image/") {
                    synthesize_image(node,matrix,normalized.viewport.width,normalized.viewport.height,&href);
                    continue;
                }
            }
        }
        if !clip_ids.contains(&node.id) && node.kind!="canvas" {
            bake_node_path(node,matrix);
        }
    }

    materialize_target_clips(&mut normalized,&world);

    for reference in &mut normalized.references {
        apply_opacity(&mut reference.properties,1.0);
        if let Some(t)=reference.properties.get("transform").map(|v|parse_transform(&unquote(v))) {
            if let Some(data)=reference.properties.get("data").map(|v|unquote(v)) {
                if let Some(out)=transform_path(&data,t){reference.properties.insert("data".into(),quote(&out));}
            }
        }
    }

    crate::semantic::prepare_scene(&normalized,revision,gate)
}

fn parent_map(scene:&Scene)->HashMap<String,String>{
    scene.relations.iter().filter(|r|r.active && r.kind.as_deref()==Some("contains")).map(|r|(r.to.clone(),r.from.clone())).collect()
}

fn viewbox_roots(scene:&Scene,parent:&HashMap<String,String>)->HashMap<String,Matrix>{
    let mut out=HashMap::new();
    for node in &scene.nodes {
        if parent.contains_key(&node.id){continue;}
        let Some(v)=node.properties.get("viewbox").map(|v|unquote(v)) else{continue;};
        if let Some(m)=viewbox_matrix(&v,node.properties.get("preserve-aspect-ratio").map(|v|unquote(v)).as_deref(),scene.viewport.width,scene.viewport.height){out.insert(node.id.clone(),m);}
    }
    out
}

fn world_matrix(id:&str,parent:&HashMap<String,String>,props:&HashMap<String,BTreeMap<String,String>>,roots:&HashMap<String,Matrix>,memo:&mut HashMap<String,Matrix>,visiting:&mut HashSet<String>)->Matrix{
    if let Some(m)=memo.get(id){return *m;}
    if !visiting.insert(id.to_string()){return Matrix::ID;}
    let local=props.get(id).and_then(|p|p.get("transform")).map(|v|parse_transform(&unquote(v))).unwrap_or(Matrix::ID);
    let base=if let Some(p)=parent.get(id){world_matrix(p,parent,props,roots,memo,visiting)}else{roots.get(id).copied().unwrap_or(Matrix::ID)};
    let m=base.mul(local);
    visiting.remove(id); memo.insert(id.to_string(),m); m
}

fn viewbox_matrix(value:&str,preserve:Option<&str>,vw:f64,vh:f64)->Option<Matrix>{
    let n=value.split(|c:char|c==','||c.is_whitespace()).filter(|s|!s.is_empty()).map(|s|s.parse::<f64>().ok()).collect::<Option<Vec<_>>>()?;
    if n.len()!=4||n[2]<=0.0||n[3]<=0.0{return None;}
    let (x,y,w,h)=(n[0],n[1],n[2],n[3]);
    let sx=vw/w; let sy=vh/h; let p=preserve.unwrap_or("xMidYMid meet");
    if p.trim()=="none"{return Some(Matrix::translate(-x*sx,-y*sy).mul(Matrix::scale(sx,sy)));}
    let slice=p.contains("slice"); let s=if slice{sx.max(sy)}else{sx.min(sy)};
    let extra_x=vw-w*s; let extra_y=vh-h*s;
    let ax=if p.contains("xMin"){0.0}else if p.contains("xMax"){extra_x}else{extra_x/2.0};
    let ay=if p.contains("YMin"){0.0}else if p.contains("YMax"){extra_y}else{extra_y/2.0};
    Some(Matrix::translate(ax-x*s,ay-y*s).mul(Matrix::scale(s,s)))
}

fn parse_transform(value:&str)->Matrix{
    let mut out=Matrix::ID; let mut rest=value.trim();
    while let Some(open)=rest.find('('){
        let name=rest[..open].trim(); let after=&rest[open+1..]; let Some(close)=after.find(')') else{break;};
        let args=after[..close].split(|c:char|c==','||c.is_whitespace()).filter(|s|!s.is_empty()).filter_map(|s|s.parse::<f64>().ok()).collect::<Vec<_>>();
        let m=match name{
            "matrix" if args.len()>=6=>Matrix{a:args[0],b:args[1],c:args[2],d:args[3],e:args[4],f:args[5]},
            "translate" if !args.is_empty()=>Matrix::translate(args[0],*args.get(1).unwrap_or(&0.0)),
            "scale" if !args.is_empty()=>Matrix::scale(args[0],*args.get(1).unwrap_or(&args[0])),
            "rotate" if !args.is_empty()=>{let r=Matrix::rotate(args[0]*PI/180.0); if args.len()>=3{Matrix::translate(args[1],args[2]).mul(r).mul(Matrix::translate(-args[1],-args[2]))}else{r}},
            "skewX" if !args.is_empty()=>Matrix::skew(args[0]*PI/180.0,0.0),
            "skewY" if !args.is_empty()=>Matrix::skew(0.0,args[0]*PI/180.0),
            _=>Matrix::ID,
        };
        out=out.mul(m); rest=&after[close+1..];
    }
    out
}

fn bake_node_path(node:&mut SceneNode,matrix:Matrix){
    let Some(path)=node_path(node) else{return;};
    if let Some(out)=transform_path(&path,matrix){node.properties.insert("data".into(),quote(&out));}
    if let Some(g)=transformed_bbox(&node.geometry,matrix){node.geometry=g;}
    node.properties.remove("transform");
}

fn node_path(node:&SceneNode)->Option<String>{
    if let Some(d)=node.properties.get("data").map(|v|unquote(v)){if !d.trim().is_empty(){return Some(d);}}
    let Geometry{x:Some(x),y:Some(y),width:Some(w),height:Some(h)}=node.geometry else{return None;};
    match node.kind.as_str(){
        "rect"|"group"|"shadow"=>Some(format!("M {x} {y} H {} V {} H {x} Z",x+w,y+h)),
        "circle"|"ellipse"=>{let rx=w/2.0;let ry=h/2.0;let cx=x+rx;let cy=y+ry;Some(format!("M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",cx-rx,cx+rx,cx-rx))},
        _=>None,
    }
}

fn transform_path(data:&str,m:Matrix)->Option<String>{
    let mut p=BezPath::from_svg(data).ok()?; p.apply_affine(m.affine()); Some(p.to_svg())
}

fn transformed_bbox(g:&Geometry,m:Matrix)->Option<Geometry>{
    let (x,y,w,h)=(g.x?,g.y?,g.width?,g.height?); let pts=[m.point(x,y),m.point(x+w,y),m.point(x,y+h),m.point(x+w,y+h)];
    let minx=pts.iter().map(|p|p.0).fold(f64::INFINITY,f64::min); let maxx=pts.iter().map(|p|p.0).fold(f64::NEG_INFINITY,f64::max);
    let miny=pts.iter().map(|p|p.1).fold(f64::INFINITY,f64::min); let maxy=pts.iter().map(|p|p.1).fold(f64::NEG_INFINITY,f64::max);
    Some(Geometry{x:Some(minx),y:Some(miny),width:Some(maxx-minx),height:Some(maxy-miny)})
}

fn preprocess_paint(node:&mut SceneNode,m:Matrix,vw:f64,vh:f64){
    let inherited_opacity=prop_number(&node.properties,"opacity").unwrap_or(1.0).clamp(0.0,1.0);
    apply_opacity(&mut node.properties,inherited_opacity);
    let kind=node.properties.get("gradient-kind").map(|v|unquote(v));
    if kind.as_deref()==Some("linear-gradient"){
        let units=node.properties.get("gradient-units").map(|v|unquote(v)).unwrap_or_else(||"objectBoundingBox".into());
        let g=&node.geometry;
        let x=g.x.unwrap_or(0.0); let y=g.y.unwrap_or(0.0); let w=g.width.unwrap_or(1.0); let h=g.height.unwrap_or(1.0);
        let p=|key:&str,axis_x:bool,default:&str| resolve_coord(node.properties.get(key).map(|v|unquote(v)).as_deref().unwrap_or(default),&units,if axis_x{x}else{y},if axis_x{w}else{h});
        let a=m.point(p("gradient-x1",true,"0%"),p("gradient-y1",false,"0%"));
        let b=m.point(p("gradient-x2",true,"100%"),p("gradient-y2",false,"0%"));
        node.properties.insert("gradient-start".into(),format!("{}px {}px",fmt(a.0),fmt(a.1)));
        node.properties.insert("gradient-end".into(),format!("{}px {}px",fmt(b.0),fmt(b.1)));
        node.properties.insert("fill".into(),"linear-gradient".into());
    } else if kind.as_deref()==Some("radial-gradient"){
        synthesize_radial(node,m,vw,vh);
    }
}

fn resolve_coord(raw:&str,units:&str,origin:f64,extent:f64)->f64{
    let t=raw.trim(); if let Some(p)=t.strip_suffix('%').and_then(|v|v.parse::<f64>().ok()){return if units=="objectBoundingBox"{origin+extent*p/100.0}else{p/100.0};}
    let n=t.trim_end_matches("px").parse::<f64>().unwrap_or(0.0); if units=="objectBoundingBox"{origin+extent*n}else{n}
}

fn synthesize_radial(node:&mut SceneNode,m:Matrix,vw:f64,vh:f64){
    let units=node.properties.get("gradient-units").map(|v|unquote(v)).unwrap_or_else(||"objectBoundingBox".into()); let g=&node.geometry;
    let x=g.x.unwrap_or(0.0);let y=g.y.unwrap_or(0.0);let w=g.width.unwrap_or(1.0);let h=g.height.unwrap_or(1.0);
    let c=|key:&str,axis_x:bool,default:&str|resolve_coord(node.properties.get(key).map(|v|unquote(v)).as_deref().unwrap_or(default),&units,if axis_x{x}else{y},if axis_x{w}else{h});
    let cx=c("gradient-cx",true,"50%");let cy=c("gradient-cy",false,"50%");let fx=c("gradient-fx",true,"50%");let fy=c("gradient-fy",false,"50%");
    let rr=node.properties.get("gradient-r").map(|v|unquote(v)).unwrap_or_else(||"50%".into()); let r=if let Some(p)=rr.trim().strip_suffix('%').and_then(|v|v.parse::<f64>().ok()){if units=="objectBoundingBox"{w.max(h)*p/100.0}else{p/100.0}}else{rr.trim_end_matches("px").parse().unwrap_or(0.5)};
    let stops=svg_stops(node.properties.get("gradient-stops").map(String::as_str).unwrap_or("")); let id=format!("__radial_{}",xml_escape(&node.id));
    let xml=format!("<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{vw}\" height=\"{vh}\"><radialGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fx=\"{fx}\" fy=\"{fy}\" gradientTransform=\"{}\">{stops}</radialGradient><rect width=\"{vw}\" height=\"{vh}\" fill=\"url(#g)\"/></pattern>",m.svg());
    node.properties.insert("pattern-ref".into(),quote(&id)); node.properties.insert("pattern-width".into(),format!("{vw}px")); node.properties.insert("pattern-height".into(),format!("{vh}px")); node.properties.insert("pattern-source".into(),quote(&xml));
}

fn svg_stops(value:&str)->String{unquote(value).split(',').filter_map(|s|{let f=s.split_whitespace().collect::<Vec<_>>();if f.len()<2{return None;}Some(format!("<stop offset=\"{}\" stop-color=\"{}\"/>",xml_escape(f[0]),xml_escape(f[1]))) }).collect::<Vec<_>>().join("")}

fn apply_opacity(props:&mut BTreeMap<String,String>,base:f64){
    let fill_op=base*prop_number(props,"fill-opacity").unwrap_or(1.0); let stroke_op=base*prop_number(props,"stroke-opacity").unwrap_or(1.0);
    if let Some(v)=props.get("fill").cloned(){if let Some(c)=with_alpha(&v,fill_op){props.insert("fill".into(),c);}}
    if let Some(v)=props.get("stroke").cloned(){if let Some(c)=with_alpha(&v,stroke_op){props.insert("stroke".into(),c);}}
    if let Some(v)=props.get("gradient-stops").cloned(){props.insert("gradient-stops".into(),opacity_stops(&v,fill_op));}
    if let Some(v)=props.get("pattern-data").cloned(){props.insert("pattern-data".into(),opacity_records(&v,fill_op,false));}
    props.remove("opacity");props.remove("fill-opacity");props.remove("stroke-opacity");
}

fn preprocess_mask(node:&mut SceneNode,m:Matrix){
    let Some(data)=node.properties.get("mask-data").cloned() else{return;}; let mode=node.properties.get("mask-type").map(|v|unquote(v)).unwrap_or_else(||"luminance".into());
    let units=node.properties.get("mask-content-units").map(|v|unquote(v)).unwrap_or_else(||"userSpaceOnUse".into());
    let bbox=if units=="objectBoundingBox"{let g=&node.geometry;Matrix::translate(g.x.unwrap_or(0.0),g.y.unwrap_or(0.0)).mul(Matrix::scale(g.width.unwrap_or(1.0),g.height.unwrap_or(1.0)))}else{Matrix::ID};
    node.properties.insert("mask-data".into(),opacity_records(&data,1.0,mode=="luminance"));
    let decoded=unquote(node.properties.get("mask-data").unwrap()); node.properties.insert("mask-data".into(),quote(&transform_records(&decoded,m.mul(bbox))));
}

fn opacity_records(value:&str,opacity:f64,luminance:bool)->String{
    let d=unquote(value); let mut out=Vec::new(); for line in d.lines(){let Some((color,path))=line.split_once('|')else{continue;}; let c=if luminance{luminance_alpha(color)}else{with_alpha(color,opacity).unwrap_or_else(||color.to_string())}; out.push(format!("{c}|{path}"));} quote(&out.join("\n"))
}
fn transform_records(value:&str,m:Matrix)->String{value.lines().filter_map(|line|{let(c,p)=line.split_once('|')?;transform_path(p,m).map(|p|format!("{c}|{p}"))}).collect::<Vec<_>>().join("\n")}

fn materialize_target_clips(scene:&mut Scene,world:&HashMap<String,Matrix>){
    let clips=scene.nodes.iter().map(|n|(n.id.clone(),n.clone())).collect::<HashMap<_,_>>(); let mut extra=Vec::new();
    for node in &mut scene.nodes {let Some(cid)=node.properties.get("clip").map(|v|unquote(v)) else{continue;};let Some(src)=clips.get(&cid) else{continue;};let units=src.properties.get("clip-units").map(|v|unquote(v)).unwrap_or_else(||"userSpaceOnUse".into());let Some(data)=src.properties.get("data").map(|v|unquote(v)) else{continue;};let base=*world.get(&node.id).unwrap_or(&Matrix::ID);let m=if units=="objectBoundingBox"{let g=&node.geometry;Matrix::translate(g.x.unwrap_or(0.0),g.y.unwrap_or(0.0)).mul(Matrix::scale(g.width.unwrap_or(1.0),g.height.unwrap_or(1.0)))}else{base};let Some(path)=transform_path(&data,m)else{continue;};let mut clone=src.clone();clone.id=format!("__clip_{}",node.id);clone.properties.insert("data".into(),quote(&path));clone.properties.remove("clip-units");node.properties.insert("clip".into(),clone.id.clone());extra.push(clone);} scene.nodes.extend(extra);
}

fn synthesize_image(node:&mut SceneNode,m:Matrix,vw:f64,vh:f64,href:&str){
    let x=prop_number(&node.properties,"source-x").unwrap_or(0.0);let y=prop_number(&node.properties,"source-y").unwrap_or(0.0);let w=prop_number(&node.properties,"source-width").unwrap_or(0.0);let h=prop_number(&node.properties,"source-height").unwrap_or(0.0);if w<=0.0||h<=0.0{return;}let id=format!("__image_{}",xml_escape(&node.id));let pa=node.properties.get("image-preserve-aspect-ratio").map(|v|unquote(v)).unwrap_or_else(||"xMidYMid meet".into());let xml=format!("<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{vw}\" height=\"{vh}\"><image href=\"{}\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"{}\" transform=\"{}\"/></pattern>",xml_escape(href),xml_escape(&pa),m.svg());full_view_pattern(node,&id,&xml,vw,vh);
}

fn synthesize_text(node:&mut SceneNode,m:Matrix,vw:f64,vh:f64){
    let text=node.properties.get("content").map(|v|unquote(v)).unwrap_or_default();if text.is_empty(){return;}let x=node.geometry.x.unwrap_or(0.0);let y=node.geometry.y.unwrap_or(0.0);let fill=node.properties.get("fill").map(|v|unquote(v)).unwrap_or_else(||"#000000".into());let size=node.properties.get("font-size").map(|v|unquote(v)).unwrap_or_else(||"16".into());let family=node.properties.get("font-family").map(|v|unquote(v)).unwrap_or_else(||"sans-serif".into());let weight=node.properties.get("font-weight").map(|v|unquote(v)).unwrap_or_else(||"normal".into());let style=node.properties.get("font-style").map(|v|unquote(v)).unwrap_or_else(||"normal".into());let anchor=node.properties.get("text-anchor").map(|v|unquote(v)).unwrap_or_else(||"start".into());let id=format!("__text_{}",xml_escape(&node.id));let xml=format!("<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{vw}\" height=\"{vh}\"><text x=\"{x}\" y=\"{y}\" fill=\"{}\" font-size=\"{}\" font-family=\"{}\" font-weight=\"{}\" font-style=\"{}\" text-anchor=\"{}\" transform=\"{}\">{}</text></pattern>",xml_escape(&fill),xml_escape(&size),xml_escape(&family),xml_escape(&weight),xml_escape(&style),xml_escape(&anchor),m.svg(),xml_escape_text(&text));full_view_pattern(node,&id,&xml,vw,vh);
}

fn full_view_pattern(node:&mut SceneNode,id:&str,xml:&str,vw:f64,vh:f64){node.geometry=Geometry{x:Some(0.0),y:Some(0.0),width:Some(vw),height:Some(vh)};node.properties.insert("data".into(),quote(&format!("M 0 0 H {vw} V {vh} H 0 Z")));node.properties.insert("pattern-ref".into(),quote(id));node.properties.insert("pattern-width".into(),format!("{vw}px"));node.properties.insert("pattern-height".into(),format!("{vh}px"));node.properties.insert("pattern-source".into(),quote(xml));}

fn prop_number(p:&BTreeMap<String,String>,k:&str)->Option<f64>{unquote(p.get(k)?).trim().trim_end_matches("px").parse().ok()}
fn with_alpha(value:&str,opacity:f64)->Option<String>{let v=unquote(value);let h=v.strip_prefix('#')?;let b=|s:&str|u8::from_str_radix(s,16).ok();let(r,g,bb,a)=match h.len(){3=>(b(&h[0..1].repeat(2))?,b(&h[1..2].repeat(2))?,b(&h[2..3].repeat(2))?,255),4=>(b(&h[0..1].repeat(2))?,b(&h[1..2].repeat(2))?,b(&h[2..3].repeat(2))?,b(&h[3..4].repeat(2))?),6=>(b(&h[0..2])?,b(&h[2..4])?,b(&h[4..6])?,255),8=>(b(&h[0..2])?,b(&h[2..4])?,b(&h[4..6])?,b(&h[6..8])?),_=>return None};Some(format!("#{r:02x}{g:02x}{bb:02x}{:02x}",(f64::from(a)*opacity.clamp(0.0,1.0)).round() as u8))}
fn luminance_alpha(value:&str)->String{let v=unquote(value);let Some(h)=v.strip_prefix('#')else{return "#ffffff00".into()};let hex=if h.len()==3{format!("{}{}{}{}{}{}",&h[0..1],&h[0..1],&h[1..2],&h[1..2],&h[2..3],&h[2..3])}else{h[..h.len().min(6)].to_string()};if hex.len()<6{return "#ffffff00".into()}let r=u8::from_str_radix(&hex[0..2],16).unwrap_or(0);let g=u8::from_str_radix(&hex[2..4],16).unwrap_or(0);let b=u8::from_str_radix(&hex[4..6],16).unwrap_or(0);let a=(0.2126*f64::from(r)+0.7152*f64::from(g)+0.0722*f64::from(b)).round() as u8;format!("#ffffff{a:02x}")}
fn opacity_stops(value:&str,o:f64)->String{unquote(value).split(',').map(|s|{let mut f=s.split_whitespace();let off=f.next().unwrap_or("0");let c=f.next().unwrap_or("#000000");format!("{off} {}",with_alpha(c,o).unwrap_or_else(||c.to_string()))}).collect::<Vec<_>>().join(", ")}
fn unquote(v:&str)->String{let v=v.trim();let Some(i)=v.strip_prefix('"').and_then(|x|x.strip_suffix('"'))else{return v.to_string()};i.replace("\\n","\n").replace("\\\"","\"").replace("\\\\","\\")}
fn quote(v:&str)->String{format!("\"{}\"",v.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n"))}
fn fmt(v:f64)->String{if v.fract().abs()<1e-9{format!("{}",v as i64)}else{format!("{v:.6}").trim_end_matches('0').trim_end_matches('.').to_string()}}
fn xml_escape(v:&str)->String{v.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
fn xml_escape_text(v:&str)->String{v.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;")}

#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn transform_parser_handles_full_svg_list(){let m=parse_transform("translate(10 20) scale(2) rotate(90)");let p=m.point(1.0,0.0);assert!((p.0-10.0).abs()<1e-6);assert!((p.1-22.0).abs()<1e-6);}
 #[test] fn viewbox_meet_centers_content(){let m=viewbox_matrix("0 0 100 100",Some("xMidYMid meet"),200.0,100.0).unwrap();let p=m.point(0.0,0.0);assert_eq!(p,(50.0,0.0));}
 #[test] fn luminance_mask_converts_gray_to_alpha(){assert_eq!(luminance_alpha("#808080"),"#ffffff80");}
}
