//! The shaders as text: what cannot be checked by running them.
//!
//! A pass that fails to compile is found by an app's own device test, but
//! only on the driver that refuses it — and the drivers disagree. SwiftShader
//! (both CI emulators) and the phones tried accepted `float noise2(vec2)`;
//! an Android tablet on ANGLE/Metal refused it, because `noise1`..`noise4`
//! are reserved in GLSL and desktop GLSL has a built-in `vec2 noise2(...)`,
//! and the app died on its GL thread at the first frame (2026-10-06). This
//! test is cheap, runs everywhere, and would have caught it before any GPU
//! did.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Names a GLSL implementation may already know. The `noise*` four are
/// built-ins in desktop GLSL; the rest are reserved by the GLSL ES 3.00
/// spec's keyword list, which a conforming compiler may reject outright.
const RESERVED: &[&str] = &[
    "noise1",
    "noise2",
    "noise3",
    "noise4",
    "sample",
    "filter",
    "active",
    "asm",
    "cast",
    "class",
    "common",
    "double",
    "enum",
    "extern",
    "external",
    "fixed",
    "goto",
    "half",
    "inline",
    "interface",
    "long",
    "namespace",
    "noinline",
    "partition",
    "public",
    "short",
    "sizeof",
    "static",
    "superp",
    "template",
    "this",
    "typedef",
    "union",
    "unsigned",
    "using",
    "volatile",
];

fn shader_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("shaders")
}

fn shaders() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = std::fs::read_dir(shader_dir())
        .expect("the shaders live next to the crates")
        .filter_map(|entry| {
            let path = entry.expect("readable").path();
            let name = path.file_name()?.to_str()?.to_string();
            if !(name.ends_with(".glsl") || name.ends_with(".frag")) {
                return None;
            }
            Some((name, std::fs::read_to_string(&path).expect("readable")))
        })
        .collect();
    found.sort();
    assert_eq!(found.len(), 8, "common.glsl and the seven passes");
    found
}

/// Every function a shader declares, by the types these shaders use.
fn declared_functions(source: &str) -> BTreeSet<String> {
    let types = ["void", "bool", "int", "uint", "float", "vec2", "vec3", "vec4", "mat2", "mat3", "mat4"];
    let mut names = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim();
        let Some((head, rest)) = line.split_once(char::is_whitespace) else { continue };
        if !types.contains(&head) {
            continue;
        }
        let Some((name, after)) = rest.trim_start().split_once('(') else { continue };
        // A declaration, not a call: the name is followed by its parameters
        // and the line ends in `{` or `;`.
        if name.is_empty() || !after.contains(')') {
            continue;
        }
        if line.ends_with('{') || line.ends_with(';') {
            names.insert(name.trim().to_string());
        }
    }
    names
}

#[test]
fn no_shader_defines_a_name_glsl_reserves() {
    for (file, source) in shaders() {
        for name in declared_functions(&source) {
            assert!(
                !RESERVED.contains(&name.as_str()),
                "{file} defines `{name}`, which GLSL reserves — some drivers refuse it \
                 (see shaders/common.glsl); call it something of its own",
            );
        }
    }
}

#[test]
fn the_helpers_are_found_where_they_are_called() {
    let (_, common) = shaders().into_iter().find(|(f, _)| f == "common.glsl").expect("common.glsl");
    let declared = declared_functions(&common);
    assert!(declared.contains("valueNoise2"), "{declared:?}");
    assert!(declared.contains("fbm"), "{declared:?}");
    // The passes call the prelude's helpers by these names; a rename that
    // misses a call site leaves a shader that only a GPU would reject.
    for (file, source) in shaders() {
        for helper in &declared {
            let calls = source.matches(&format!("{helper}(")).count();
            if calls > 0 && file != "common.glsl" {
                assert!(
                    common.contains(&format!("{helper}(")),
                    "{file} calls `{helper}`, which common.glsl does not define",
                );
            }
        }
    }
}
