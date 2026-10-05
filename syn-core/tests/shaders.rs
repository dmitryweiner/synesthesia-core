//! The picture's shaders ship from this repository (synesthesia
//! PLAN-CORE.md C10): every pass the renderers draw is here, GLSL ES 3.00.

const PASSES: [&str; 8] = [
    "common.glsl",
    "seed.frag",
    "react.frag",
    "paramfield.frag",
    "velocity.frag",
    "advect.frag",
    "inject.frag",
    "display.frag",
];

#[test]
fn every_pass_is_here() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../shaders");
    for f in PASSES {
        let src = std::fs::read_to_string(dir.join(f)).unwrap_or_else(|e| panic!("shaders/{f}: {e}"));
        assert!(src.contains("void main") || f == "common.glsl", "{f} has no main()");
    }
}
