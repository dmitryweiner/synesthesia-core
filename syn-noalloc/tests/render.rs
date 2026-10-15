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

/// Layers playing: events every block, the pool full and stealing, beds
/// looping, synth drums, dry and wet — still nothing allocated or freed.
#[test]
fn layers_allocate_nothing() {
    use syn_core::seq::{Layer, Mode, Sample, SampleBank, SeqProgram, SeqState};
    let tone = |n: usize| (0..n).map(|i| ((i * 37) % 2000) as i16 - 1000).collect::<Vec<i16>>();
    let bank = SampleBank::from_pcm(vec![
        ("long".into(), vec![Sample { sr: 24000, data: tone(48000), mode: Mode::Hits }]),
        ("loop".into(), vec![Sample { sr: 32000, data: tone(8000), mode: Mode::Bed }]),
    ]);
    let l = |sound: &str, pattern: &str, mode, dry| Layer {
        on: true,
        sound: sound.into(),
        pattern: pattern.into(),
        density: 0.8,
        mode,
        dry,
        ..Layer::default()
    };
    let seq = SeqState {
        cps: 1.7,
        layers: vec![
            l("long", "0*32 , [0 1]*16?", Mode::Hits, false),
            l("loop", "", Mode::Bed, false),
            l("bd", "0(5,8) , ~ sd , hh*16 , <oh cp>*4", Mode::Hits, true),
            l("long", "<0 [0 0] 0(3,8,2)> | 0*8", Mode::Hits, false),
        ],
    };
    let mut buf = vec![0.0f32; BLOCK];
    for p in [&presets()[0].state, &presets()[3].state] {
        // The layers in the point, and LFOs aimed at them: the engine reads
        // and modulates their numbers every block.
        let mut p = syn_core::state::AppState { seq: seq.clone(), ..p.clone() };
        for (target, param) in [("seq", "cps"), ("seq.0", "gain"), ("seq.2", "speed"), ("seq.3", "density")] {
            p.modulation.routes.push(syn_core::ModRoute {
                src: 1,
                target: target.into(),
                param: param.into(),
                depth: 0.6,
                exp: false,
            });
        }
        let mut e = Engine::new(48000.0, &p, 1);
        e.set_seq(Box::new(SeqProgram::new(&p.seq, Some(&bank))));
        e.render(&mut buf);
        let ((), n) = count(|| {
            for _ in 0..3000 {
                e.render(&mut buf);
            }
        });
        assert!(e.sequencer().stolen > 0, "the pool should have been full");
        assert_eq!(n, 0, "{n} allocations in 3000 blocks with layers");
    }
}
