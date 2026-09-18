use srng::runtime::{execute_json, RuntimeOptions};
use srng_renderer::{cpu, prepare_scene, Command, FilterInput, FilterPrimitive, RevisionGate};

fn scene(source:&str)->srng::runtime::Scene{
    let ir=srng::compile_to_json(source,"filter-graph-v06.srng");
    let mut options=RuntimeOptions::default();options.viewport_width=64.0;options.viewport_height=64.0;
    execute_json(&ir,&options).expect("runtime scene")
}

#[test]
fn multi_stage_fixture_keeps_named_results_and_source_alpha(){
    let source=include_str!("fixtures/filter_graph_multi_stage.srng");
    let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene(source),revision,&gate);
    let graph=prepared.commands.iter().find_map(|command|match command{Command::PushFilter{graph}=>Some(graph),_=>None}).expect("filter graph");
    assert_eq!(graph.nodes.len(),5);
    assert!(matches!(&graph.nodes[0].op,FilterPrimitive::GaussianBlur{input:FilterInput::SourceAlpha,..}));
    assert!(matches!(&graph.nodes[3].op,FilterPrimitive::Composite{input:FilterInput::Named(a),input2:FilterInput::Named(b),..} if a=="blue"&&b=="shadow_alpha"));
    assert_eq!(graph.output,FilterInput::Named("final".into()));
    assert!(!prepared.diagnostics.iter().any(|d|d.severity=="error"),"{:?}",prepared.diagnostics);
}

#[test]
fn cpu_executes_multi_stage_graph_without_affecting_unrelated_shape(){
    let source=include_str!("fixtures/filter_graph_multi_stage.srng");
    let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene(source),revision,&gate);let output=cpu::render(&prepared);
    assert!(!output.diagnostics.iter().any(|d|d.severity=="error"),"{:?}",output.diagnostics);
    let pixel=|x:usize,y:usize|{let i=(y*usize::from(output.width)+x)*4;[output.pixels[i],output.pixels[i+1],output.pixels[i+2],output.pixels[i+3]]};
    let unaffected=pixel(52,52);assert!(unaffected[1]>0&&unaffected[3]>0,"unrelated green shape was corrupted: {unaffected:?}");
}

#[test]
fn unsupported_filter_node_is_diagnosed_and_bypassed(){
    let source=r#"
srng 0.1;
rect a { position: 4px 4px; size: 12px 12px; fill: #ff0000ff; filter-graph: "feGaussianBlur(0)->a; feTurbulence(in=a)->u"; filter-units: "userSpaceOnUse"; filter-x: 0; filter-y: 0; filter-width: 64; filter-height: 64; }
rect b { position: 40px 40px; size: 12px 12px; fill: #00ff00ff; }
"#;
    let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene(source),revision,&gate);
    assert!(prepared.diagnostics.iter().any(|d|d.code=="G241"));
    let output=cpu::render(&prepared);assert!(!output.diagnostics.iter().any(|d|d.severity=="error"),"{:?}",output.diagnostics);
    let i=(46usize*64+46)*4;assert!(output.pixels[i+1]>0&&output.pixels[i+3]>0);
}

#[test]
fn filter_region_clips_filter_output(){
    let source=r#"
srng 0.1;
rect a { position: 0px 0px; size: 32px 32px; fill: #ff0000ff; filter-graph: "feOffset(dx=0 dy=0)->o"; filter-units: "userSpaceOnUse"; filter-x: 8; filter-y: 8; filter-width: 8; filter-height: 8; }
"#;
    let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene(source),revision,&gate);let output=cpu::render(&prepared);
    let alpha=|x:usize,y:usize|output.pixels[(y*64+x)*4+3];
    assert_eq!(alpha(4,4),0);assert!(alpha(10,10)>0);assert_eq!(alpha(20,20),0);
}
