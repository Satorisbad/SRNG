use crate::{PreparedScene, RevisionGate};
use srng::runtime::Scene;

/// Deterministic text fallback used only when no pre-shaped outline data is
/// present. Semantic font metadata is retained for future/full shapers.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();
    for node in &mut normalized.nodes {
        if !node.active || node.kind != "text" || node.properties.contains_key("data") {
            continue;
        }
        let content = node.properties.get("content").map(|v| unquote(v)).unwrap_or_default();
        if content.is_empty() { continue; }
        let size = node.properties.get("font-size").map(|v| unquote(v)).and_then(|v| parse_number(&v)).unwrap_or(16.0).max(1.0);
        let anchor = node.properties.get("text-anchor").map(|v| unquote(v)).unwrap_or_else(|| "start".to_string());
        let weight = node.properties.get("font-weight").map(|v| unquote(v)).unwrap_or_else(|| "normal".into());
        let style = node.properties.get("font-style").map(|v| unquote(v)).unwrap_or_else(|| "normal".into());
        let letter_spacing = node.properties.get("letter-spacing").map(|v| unquote(v)).and_then(|v| parse_number(&v)).unwrap_or(0.0);
        let baseline_x = node.geometry.x.unwrap_or(0.0);
        let baseline_y = node.geometry.y.unwrap_or(size);
        let bold = weight.parse::<u16>().map(|w| w >= 600).unwrap_or(matches!(weight.as_str(), "bold" | "bolder"));
        let italic = matches!(style.as_str(), "italic" | "oblique");
        let (path, width) = bitmap_text_path(&content, baseline_x, baseline_y, size, &anchor, letter_spacing, bold, italic);
        if path.is_empty() { continue; }
        node.properties.insert("data".into(), quote(&path));
        node.properties.insert("text-fallback".into(), quote("builtin-vector"));
        node.geometry.x = Some(match anchor.as_str() { "middle" => baseline_x - width / 2.0, "end" => baseline_x - width, _ => baseline_x });
        node.geometry.y = Some(baseline_y - size);
        node.geometry.width = Some(width);
        node.geometry.height = Some(size);
    }
    crate::semantic_v6::prepare_scene(&normalized, revision, gate)
}

fn bitmap_text_path(text:&str,x:f64,baseline_y:f64,size:f64,anchor:&str,letter_spacing:f64,bold:bool,italic:bool)->(String,f64){
    let cell=size/7.0;let base_advance=cell*6.0;let chars=text.chars().collect::<Vec<_>>();let width=if chars.is_empty(){0.0}else{chars.len() as f64*base_advance+(chars.len().saturating_sub(1) as f64)*letter_spacing};let start_x=match anchor{"middle"=>x-width/2.0,"end"=>x-width,_=>x};let top=baseline_y-size;let mut out=String::new();let mut cursor=start_x;
    for ch in chars {let rows=glyph(ch);for(row,bits)in rows.iter().copied().enumerate(){for col in 0..5{if bits&(1<<(4-col))==0{continue;}let slant=if italic{(6-row) as f64*cell*0.18}else{0.0};let x0=cursor+col as f64*cell+slant;let y0=top+row as f64*cell;let extra=if bold{cell*0.22}else{0.0};let x1=x0+cell+extra;let y1=y0+cell;out.push_str(&format!("M {} {} H {} V {} H {} Z ",fmt(x0),fmt(y0),fmt(x1),fmt(y1),fmt(x0)));}}cursor+=base_advance+letter_spacing;}
    (out.trim().to_string(),width)
}

fn glyph(ch:char)->[u8;7]{match ch.to_ascii_uppercase(){
'A'=>[14,17,17,31,17,17,17],'B'=>[30,17,17,30,17,17,30],'C'=>[15,16,16,16,16,16,15],'D'=>[30,17,17,17,17,17,30],'E'=>[31,16,16,30,16,16,31],'F'=>[31,16,16,30,16,16,16],'G'=>[15,16,16,23,17,17,15],'H'=>[17,17,17,31,17,17,17],'I'=>[31,4,4,4,4,4,31],'J'=>[7,2,2,2,18,18,12],'K'=>[17,18,20,24,20,18,17],'L'=>[16,16,16,16,16,16,31],'M'=>[17,27,21,21,17,17,17],'N'=>[17,25,21,19,17,17,17],'O'=>[14,17,17,17,17,17,14],'P'=>[30,17,17,30,16,16,16],'Q'=>[14,17,17,17,21,18,13],'R'=>[30,17,17,30,20,18,17],'S'=>[15,16,16,14,1,1,30],'T'=>[31,4,4,4,4,4,4],'U'=>[17,17,17,17,17,17,14],'V'=>[17,17,17,17,17,10,4],'W'=>[17,17,17,21,21,21,10],'X'=>[17,17,10,4,10,17,17],'Y'=>[17,17,10,4,4,4,4],'Z'=>[31,1,2,4,8,16,31],
'0'=>[14,17,19,21,25,17,14],'1'=>[4,12,4,4,4,4,14],'2'=>[14,17,1,2,4,8,31],'3'=>[30,1,1,14,1,1,30],'4'=>[2,6,10,18,31,2,2],'5'=>[31,16,16,30,1,1,30],'6'=>[14,16,16,30,17,17,14],'7'=>[31,1,2,4,8,8,8],'8'=>[14,17,17,14,17,17,14],'9'=>[14,17,17,15,1,1,14],
' '=>[0,0,0,0,0,0,0],'-'=>[0,0,0,31,0,0,0],'_'=>[0,0,0,0,0,0,31],'.'=>[0,0,0,0,0,0,4],','=>[0,0,0,0,0,4,8],':'=>[0,4,0,0,4,0,0],';'=>[0,4,0,0,4,4,8],'!'=>[4,4,4,4,4,0,4],'?'=>[14,17,1,2,4,0,4],'/'=>[1,2,4,8,16,0,0],'\\'=>[16,8,4,2,1,0,0],'('=>[2,4,8,8,8,4,2],')'=>[8,4,2,2,2,4,8],'+'=>[0,4,4,31,4,4,0],'='=>[0,31,0,31,0,0,0],_=>[31,17,5,4,20,17,31]}}
fn parse_number(value:&str)->Option<f64>{value.trim().trim_end_matches("px").parse().ok()}
fn unquote(value:&str)->String{let value=value.trim();let Some(inner)=value.strip_prefix('"').and_then(|v|v.strip_suffix('"'))else{return value.to_string()};inner.replace("\\n","\n").replace("\\\"","\"").replace("\\\\","\\")}
fn quote(value:&str)->String{format!("\"{}\"",value.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n"))}
fn fmt(value:f64)->String{if value.fract().abs()<1e-9{format!("{}",value as i64)}else{format!("{value:.6}").trim_end_matches('0').trim_end_matches('.').to_string()}}

#[cfg(test)]mod tests{use super::*;#[test]fn fallback_text_generates_vector_geometry(){let(path,width)=bitmap_text_path("SRNG",2.0,18.0,16.0,"start",0.0,false,false);assert!(path.contains('M'));assert!(width>0.0);}#[test]fn style_and_spacing_change_fallback(){let(a,wa)=bitmap_text_path("AA",0.0,20.0,14.0,"start",0.0,false,false);let(b,wb)=bitmap_text_path("AA",0.0,20.0,14.0,"start",2.0,true,true);assert_ne!(a,b);assert!(wb>wa);}}
