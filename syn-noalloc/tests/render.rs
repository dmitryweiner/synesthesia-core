//! The render path allocates nothing, for every preset.

use syn_core::state::presets;
use syn_core::{Engine, BLOCK};
use syn_noalloc::{count, Counting};

#[global_allocator]
static A: Counting = Counting;

#[test]
fn engine_render_allocates_nothing() {
    let mut buf = vec![0.0f32; BLOCK];
    for p in presets() {
        let mut e = Engine::new(48000.0, &p.state, 1);
        e.render(&mut buf);
        let ((), n) = count(|| {
            for _ in 0..400 {
                e.render(&mut buf);
            }
        });
        assert_eq!(n, 0, "{}: {n} allocations in 400 blocks", p.name);
    }
}
