//! The Settings page, derived from the schema rather than written out
//! (synesthesia-android PLAN.md, decision 3: the page is generated from
//! `schema()`).
//!
//! Everything the page can change is a gene — the genome is exactly "what
//! evolution may touch", which is the same list — so a control *is* a
//! [`GeneDef`](crate::schema::GeneDef): it already carries the label, the
//! range, the step, whether it is logarithmic, and the switch that gates it.
//! What this module adds is the arrangement: which section a gene belongs to,
//! what that section is called, which switch turns it on, and what a choice's
//! options are called.
//!
//! No label is written here. The section titles are the schema's own: an FX
//! module is titled by its on-switch (`fx.filterOn` is labelled "Filter"), a
//! formula by its enable gene ("Harmonic Sum"), a route by `route.N.on`
//! ("Route 1"), a card by its `title`. Only the LFO sections are numbered
//! from their group id, because the schema names no LFO as a whole.

use crate::genome::codec::{decode_genome, encode_genome};
use crate::genome::genes::{gene_from_value, gene_index_of, genes, is_gene_active, read_value, Genome};
use crate::modmatrix::LfoShape;
use crate::schema::{card_def, formula_def, schema, GeneDef, GeneKind};
use crate::state::AppState;

/// Which tab a section sits on. The sound's parameters and the picture's are
/// the two halves of a point, and the page shows them that way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Sound,
    Picture,
}

/// One group of controls, as the page shows it.
pub struct Section {
    /// The gene group, or an FX module's on-key — what the controls share.
    pub id: String,
    pub title: String,
    /// What this thing *is*, in the schema's own words: a formula's and a
    /// card's one-line description ("Σ aₙ(t) sin(2π n f t)", "Lorenz ODE
    /// mapped to freq/amp"). Empty where the schema has none — the FX
    /// modules, the LFOs, the routes and the couplings name themselves.
    pub description: String,
    pub tab: Tab,
    /// The switch that turns the whole section on, when it has one (a
    /// formula's `enabled`, a card's `on`, an FX module's, a route's).
    pub toggle: Option<&'static GeneDef>,
    /// Everything else, in the schema's order.
    pub controls: Vec<&'static GeneDef>,
}

/// Further reading about a formula or a card: the English Wikipedia article
/// about the thing itself, where there is one.
///
/// Editorial, not dumped — the web app has no links, so these are written
/// here rather than taken from `assets/`, and every one of them was checked
/// against the Wikipedia API on 2026-10-05. Where `../chromaflux` (the
/// picture's origin) links the same things, it links the same articles;
/// `../formula-synth` (the sound's) links none. Where the thing has no article
/// (Velvet noise), or where an article would be about the word and not the
/// method (the noise beds, the rain), there is none, and an app shows no
/// link. If the web app ever grows links of its own, the dump wins.
pub fn article(section_id: &str) -> Option<String> {
    const WIKI: &str = "https://en.wikipedia.org/wiki/";
    let page = match section_id {
        "a.additive" => "Additive_synthesis",
        "a.fm" | "a.bell" => "Frequency_modulation_synthesis",
        "a.pm" => "Phase_modulation",
        "a.logistic" => "Logistic_map",
        "a.lorenz" => "Lorenz_system",
        "a.rossler" => "R%C3%B6ssler_attractor",
        "a.gliss" => "Glissando",
        "a.shepard" => "Shepard_tone",
        "a.risset" => "Jean-Claude_Risset",
        "a.karplus" => "Karplus%E2%80%93Strong_string_synthesis",
        "a.beats" => "Beat_(acoustics)",
        "a.dist" => "Distortion_(music)",
        "a.quasi" => "Quasiperiodicity",
        "a.noiselp" => "White_noise",
        "a.pinknoise" => "Pink_noise",
        "a.brownnoise" => "Brownian_noise",
        "a.bytebeat" => "Bytebeat",
        "a.tanpura" => "Tanpura",
        "a.bowl" => "Standing_bell",
        "v.reaction" => "Reaction%E2%80%93diffusion_system",
        "v.fieldVariation" => "Fractional_Brownian_motion",
        "v.flow" => "Advection",
        // Velvet noise has no article; the ocean and the rain are noise beds,
        // and an article about either word would be about the weather.
        _ => return None,
    };
    Some(format!("{WIKI}{page}"))
}

/// What a choice control offers, in value order: the names the web app shows.
pub fn options(gene: &GeneDef) -> Vec<String> {
    if gene.kind != GeneKind::Choice {
        return Vec::new();
    }
    let s = schema();
    match gene.id.as_str() {
        "fx.filterType" => s.filter_types.clone(),
        "fx.chorusMode" => s.chorus_modes.clone(),
        "fx.phaserStages" => s.phaser_stages.iter().map(|n| format!("{n}")).collect(),
        id if id.starts_with("lfo.") => s.lfo_shapes.clone(),
        id if id.ends_with(".src") => (0..s.lfo_count).map(|i| format!("LFO {}", i + 1)).collect(),
        id if id.ends_with(".target") => s.mod_targets.iter().map(|t| t.label.clone()).collect(),
        _ => {
            // A card's own select — the palette, for one.
            let card = gene.id.strip_prefix("v.").and_then(|rest| rest.split_once('.'));
            let select = card
                .and_then(|(id, key)| card_def(id).map(|c| (c, key)))
                .and_then(|(c, key)| c.selects.iter().find(|s| s.k == key));
            select.map_or_else(Vec::new, |s| s.options.iter().map(|o| o.label.clone()).collect())
        }
    }
}

/// A control's label without the section's name: "Reaction · Feed" reads as
/// "Feed" under a section already titled "Reaction".
///
/// The schema writes a label one of three ways, and this unpicks each: a dot
/// separates a group from its control ("Harmonic Sum · Gain"); a numbered
/// group simply prefixes it ("LFO 1 rate", "Route 1 depth"); and an FX
/// parameter's label already reads on its own ("Filter cutoff"), so it is
/// left alone.
pub fn short_label(section: &Section, gene: &GeneDef) -> String {
    if let Some((_, rest)) = gene.label.split_once(" · ") {
        return rest.to_string();
    }
    let numbered = section.title.ends_with(|c: char| c.is_ascii_digit());
    let prefix = format!("{} ", section.title);
    match gene.label.strip_prefix(&prefix) {
        Some(rest) if numbered => rest.to_string(),
        _ => gene.label.clone(),
    }
}

/// Which FX module a parameter belongs to. The schema maps the modulatable
/// ones; the rest — the selects and the reverb's decay — go by their name,
/// which is the module's own prefix (`reverbDecay` is the reverb's).
fn fx_module(key: &str) -> Option<&'static str> {
    let s = schema();
    if let Some(module) = s.fx_param_module.get(key) {
        return Some(module.as_str());
    }
    s.fx_on_keys.iter().find(|on| key.starts_with(on.strip_suffix("On").unwrap_or(on))).map(String::as_str)
}

/// "Additive · Σ aₙ(t) sin(2π n f t)" — the schema's own two words about a
/// thing, joined as the web app shows them.
fn describe(tag: &str, desc: &str) -> String {
    match (tag.is_empty(), desc.is_empty()) {
        (true, true) => String::new(),
        (true, false) => desc.to_string(),
        (false, true) => tag.to_string(),
        (false, false) => format!("{tag} · {desc}"),
    }
}

fn gene(id: &str) -> Option<&'static GeneDef> {
    schema().genes.iter().find(|g| g.id == id)
}

fn genes_in(group: &str) -> Vec<&'static GeneDef> {
    schema().genes.iter().filter(|g| g.group == group).collect()
}

/// The page: every section, in the order it is shown.
///
/// Sound: the FX modules, then the formulas, then the LFOs and the routes.
/// Picture: the cards, then the sound → image couplings.
pub fn sections() -> Vec<Section> {
    let s = schema();
    let mut out = Vec::new();

    // The FX genes are one group; the schema says which module each belongs
    // to, so the page can show a module at a time, as the web app does.
    for on_key in &s.fx_on_keys {
        let toggle = gene(&format!("fx.{on_key}"));
        let controls = genes_in("fx")
            .into_iter()
            .filter(|g| {
                let key = g.id.strip_prefix("fx.").unwrap_or(&g.id);
                // The on-switches are the sections' own; a module's controls
                // are everything else that belongs to it.
                !s.fx_on_keys.iter().any(|k| k == key) && fx_module(key) == Some(on_key.as_str())
            })
            .collect();
        if let Some(toggle) = toggle {
            out.push(Section {
                id: on_key.clone(),
                title: toggle.label.clone(),
                description: String::new(),
                tab: Tab::Sound,
                toggle: Some(toggle),
                controls,
            });
        }
    }

    for id in &s.formula_ids {
        let group = format!("a.{id}");
        let mut controls = genes_in(&group);
        let toggle =
            controls.iter().position(|g| g.id == format!("{group}.enabled")).map(|i| controls.remove(i));
        let title = toggle.map_or_else(|| id.clone(), |t| t.label.clone());
        let description = formula_def(id).map_or_else(String::new, |f| describe(&f.tag, &f.desc));
        out.push(Section { id: group, title, description, tab: Tab::Sound, toggle, controls });
    }

    for i in 0..s.lfo_count {
        // The only title the schema does not hold: an LFO is named by its
        // number, which its genes' labels agree with ("LFO 1 rate").
        out.push(Section {
            id: format!("lfo.{i}"),
            title: format!("LFO {}", i + 1),
            description: String::new(),
            tab: Tab::Sound,
            toggle: None,
            controls: genes_in(&format!("lfo.{i}")),
        });
    }

    for i in 0..s.route_slots {
        let group = format!("route.{i}");
        let mut controls = genes_in(&group);
        let toggle = controls.iter().position(|g| g.id == format!("{group}.on")).map(|i| controls.remove(i));
        let title = toggle.map_or_else(|| group.clone(), |t| t.label.clone());
        out.push(Section { id: group, title, description: String::new(), tab: Tab::Sound, toggle, controls });
    }

    for card in &s.cards {
        let group = format!("v.{}", card.id);
        let mut controls = genes_in(&group);
        let toggle = controls.iter().position(|g| g.id == format!("{group}.on")).map(|i| controls.remove(i));
        out.push(Section {
            id: group,
            title: card.title.clone(),
            description: describe(&card.tag, &card.desc),
            tab: Tab::Picture,
            toggle,
            controls,
        });
    }

    let couplings = genes_in("coupling");
    if let Some(first) = couplings.first() {
        // "Coupling · loudToFlow" → the section is what they share.
        let title = first
            .label
            .split_once(" · ")
            .map_or_else(|| "Coupling".to_string(), |(head, _)| head.to_string());
        out.push(Section {
            id: "coupling".into(),
            title,
            description: String::new(),
            tab: Tab::Picture,
            toggle: None,
            controls: couplings,
        });
    }

    out
}

/// A point being edited by hand, as the Settings page edits it.
///
/// The page changes genes, so the edit *is* a genome — plus the two things a
/// genome does not carry: the user's volume and the point's name. Nothing is
/// repaired on the way out (no formula at all, or a coupling below the floor,
/// is what the user chose); the next 👍/👎 repairs as usual.
pub struct Edit {
    genome: Genome,
    master_gain: f64,
    name: Option<String>,
}

impl Edit {
    pub fn new(point: &AppState) -> Self {
        Edit {
            genome: encode_genome(point),
            master_gain: point.audio.master_gain,
            name: point.preset_name.clone(),
        }
    }

    /// The point as edited, for the sound to follow and for the session to
    /// commit.
    pub fn point(&self) -> AppState {
        let mut out = decode_genome(&self.genome);
        out.audio.master_gain = self.master_gain;
        out.preset_name = self.name.clone();
        out
    }

    /// A control's value in its own units — hertz, decibels, 0..1, or a
    /// choice's index. Unknown ids read as zero rather than panicking across
    /// an FFI.
    pub fn value(&self, id: &str) -> f64 {
        if gene_index_of(id).is_some() {
            read_value(&self.genome, id)
        } else {
            0.0
        }
    }

    pub fn set_value(&mut self, id: &str, v: f64) {
        if let Some(i) = gene_index_of(id) {
            self.genome[i] = gene_from_value(&genes()[i], v);
        }
    }

    /// False when the control's switch is off, so the page can grey it out:
    /// the value is still there, it simply does nothing.
    pub fn active(&self, id: &str) -> bool {
        gene_index_of(id).is_some_and(|i| is_gene_active(&self.genome, i))
    }

    /// Everything on one tab back to where a new point starts — ⚙'s "begin
    /// again from nothing", asked for the sound and the picture apart
    /// (2026-10-06). The other tab is left exactly as it was.
    ///
    /// The values are a fresh [`AppState`]'s, not a second table written
    /// here: every formula and card off at its slider defaults, the default
    /// FX, four resting LFOs, no routes. So a reset leaves the point the
    /// sound of silence on that half — which is what a clean sheet is — and
    /// the app's own undo takes it back, because closing the page is one step.
    pub fn reset(&mut self, tab: Tab) {
        let fresh = AppState::new();
        let genome = encode_genome(&fresh);
        for section in sections().iter().filter(|s| s.tab == tab) {
            for gene in section.toggle.iter().copied().chain(section.controls.iter().copied()) {
                if let Some(i) = gene_index_of(&gene.id) {
                    self.genome[i] = genome[i];
                }
            }
        }
        // Not a gene, and the sound's: the picture's reset must not move it.
        if tab == Tab::Sound {
            self.master_gain = fresh.audio.master_gain;
        }
    }

    pub fn master_gain(&self) -> f64 {
        self.master_gain
    }

    pub fn set_master_gain(&mut self, v: f64) {
        self.master_gain = v;
    }
}

/// The LFO shape a choice index means, for an app that wants to show it.
pub fn lfo_shape(index: f64) -> LfoShape {
    match schema().lfo_shapes.get(index.max(0.0) as usize).map(String::as_str) {
        Some("triangle") => LfoShape::Triangle,
        Some("saw") => LfoShape::Saw,
        Some("square") => LfoShape::Square,
        Some("random") => LfoShape::Random,
        Some("pink") => LfoShape::Pink,
        _ => LfoShape::Sine,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::presets;
    use std::collections::BTreeSet;

    /// A tab's reset is that tab's: the other half of the point is the same
    /// point afterwards, down to the gene.
    #[test]
    fn a_reset_touches_one_tab_and_leaves_the_other_alone() {
        for tab in [Tab::Sound, Tab::Picture] {
            let point = &presets()[0].state;
            let mut edit = Edit::new(point);
            edit.set_master_gain(0.42);
            let before = Edit::new(point);
            edit.reset(tab);

            let fresh = Edit::new(&AppState::new());
            for section in sections() {
                for gene in section.toggle.iter().copied().chain(section.controls.iter().copied()) {
                    let (got, want) = (
                        edit.value(&gene.id),
                        if section.tab == tab { fresh.value(&gene.id) } else { before.value(&gene.id) },
                    );
                    assert!((got - want).abs() < 1e-9, "{tab:?}: {} is {got}, wanted {want}", gene.id,);
                }
            }
        }
    }

    /// The volume is not a gene and belongs to the sound.
    #[test]
    fn only_the_sounds_reset_moves_the_volume() {
        let point = &presets()[0].state;
        let mut edit = Edit::new(point);
        edit.set_master_gain(0.42);
        edit.reset(Tab::Picture);
        assert_eq!(edit.master_gain(), 0.42);
        edit.reset(Tab::Sound);
        assert_eq!(edit.master_gain(), AppState::new().audio.master_gain);
    }

    /// Both resets together are a new point: that is what "from nothing"
    /// means, and it is the same nothing `AppState::new()` starts from.
    ///
    /// Compared as genomes, not as points: a point that has been through the
    /// genome comes back with the last bits moved (0.35 as
    /// 0.35000000000000003), which is what `same_genome` is for.
    #[test]
    fn resetting_both_tabs_is_a_fresh_point() {
        let mut edit = Edit::new(&presets()[3].state);
        edit.reset(Tab::Sound);
        edit.reset(Tab::Picture);
        let fresh = AppState::new();
        assert!(crate::genome::evolve::same_genome(&encode_genome(&edit.point()), &encode_genome(&fresh),));
        assert_eq!(edit.point().audio.master_gain, fresh.audio.master_gain);
        assert!(edit.point().enabled_formulas().is_empty(), "a clean sheet makes no sound yet");
    }

    #[test]
    fn every_gene_is_on_the_page_exactly_once() {
        let mut seen: Vec<&str> = Vec::new();
        for s in sections() {
            seen.extend(s.toggle.iter().map(|g| g.id.as_str()));
            seen.extend(s.controls.iter().map(|g| g.id.as_str()));
        }
        let unique: BTreeSet<&str> = seen.iter().copied().collect();
        assert_eq!(seen.len(), unique.len(), "a gene is shown twice");
        let all: BTreeSet<&str> = schema().genes.iter().map(|g| g.id.as_str()).collect();
        let missing: Vec<&&str> = all.difference(&unique).collect();
        assert!(missing.is_empty(), "genes with nowhere to be edited: {missing:?}");
        assert_eq!(unique.len(), schema().genes.len());
    }

    #[test]
    fn the_sections_are_titled_by_the_schema_itself() {
        let page = sections();
        let titled = |id: &str| page.iter().find(|s| s.id == id).map(|s| s.title.clone());
        // An FX module is titled by its on-switch, a formula by its enable
        // gene, a route by its own, a card by its title.
        assert_eq!(titled("filterOn").as_deref(), Some("Filter"));
        assert_eq!(titled("a.additive").as_deref(), Some("Harmonic Sum"));
        assert_eq!(titled("route.0").as_deref(), Some("Route 1"));
        assert_eq!(titled("v.reaction").as_deref(), Some(card_def("reaction").unwrap().title.as_str()));
        assert_eq!(titled("lfo.0").as_deref(), Some("LFO 1"));
        assert_eq!(titled("coupling").as_deref(), Some("Coupling"));
    }

    #[test]
    fn a_section_carries_its_switch_and_its_own_parameters() {
        let page = sections();
        let filter = page.iter().find(|s| s.id == "filterOn").expect("the filter");
        assert_eq!(filter.toggle.map(|g| g.id.as_str()), Some("fx.filterOn"));
        assert!(filter.controls.iter().any(|g| g.id == "fx.filterFreq"));
        assert!(!filter.controls.iter().any(|g| g.id == "fx.chorusRate"), "another module's");
        assert!(filter.controls.iter().all(|g| g.active_if.as_deref() == Some("fx.filterOn")));

        // The new instruments arrived on the page with nothing added here.
        let tanpura = page.iter().find(|s| s.id == "a.tanpura").expect("the tanpura");
        assert!(tanpura.controls.iter().any(|g| g.id == "a.tanpura.tanJawari"));
        let delay = page.iter().find(|s| s.id == "delayOn").expect("the delay");
        assert!(delay.controls.iter().any(|g| g.id == "fx.delayShimmer"), "the shimmer is editable");
    }

    #[test]
    fn further_reading_is_offered_only_where_there_is_some() {
        // Every id with an article is a section on the page: a typo here
        // would be a link nobody could reach.
        let ids: BTreeSet<String> = sections().iter().map(|s| s.id.clone()).collect();
        let mut linked = 0;
        for id in &ids {
            if let Some(url) = article(id) {
                assert!(url.starts_with("https://en.wikipedia.org/wiki/"), "{id}: {url}");
                assert!(!url.ends_with('/'), "{id} links to nothing");
                linked += 1;
            }
        }
        assert!(linked > 15, "most of the formulas have an article: {linked}");
        assert!(article("a.additive").is_some_and(|u| u.ends_with("Additive_synthesis")));
        assert!(article("a.tanpura").is_some_and(|u| u.ends_with("Tanpura")));
        assert!(article("v.reaction").is_some());
        // Nothing to point at, so nothing is offered.
        assert!(article("a.velvetnoise").is_none());
        assert!(article("filterOn").is_none());
        assert!(article("lfo.0").is_none());
        assert!(article("nonsense").is_none());
    }

    #[test]
    fn a_section_says_what_the_thing_is_where_the_schema_knows() {
        let page = sections();
        let described = |id: &str| page.iter().find(|s| s.id == id).map(|s| s.description.clone());
        // A formula's and a card's own words, as the web app shows them.
        let additive = described("a.additive").expect("the additive section");
        assert!(additive.contains("Additive") && additive.contains("sin"), "{additive}");
        assert!(described("a.tanpura").is_some_and(|d| !d.is_empty()), "the newest instrument too");
        assert!(described("v.reaction").is_some_and(|d| !d.is_empty()));
        // The rest name themselves; the schema has nothing to add.
        assert_eq!(described("filterOn").as_deref(), Some(""));
        assert_eq!(described("lfo.0").as_deref(), Some(""));
        assert_eq!(described("coupling").as_deref(), Some(""));
    }

    #[test]
    fn a_choice_knows_what_it_offers() {
        let page = sections();
        let find = |id: &str| {
            page.iter()
                .flat_map(|s| s.controls.iter().chain(s.toggle.iter()))
                .find(|g| g.id == id)
                .copied()
                .unwrap_or_else(|| panic!("no gene {id}"))
        };
        assert_eq!(options(find("fx.filterType")), schema().filter_types);
        assert_eq!(options(find("lfo.0.shape")), schema().lfo_shapes);
        assert_eq!(options(find("fx.phaserStages")), ["2", "4", "6", "8"]);
        assert_eq!(options(find("route.0.src")), ["LFO 1", "LFO 2", "LFO 3", "LFO 4"]);
        assert_eq!(options(find("route.0.target")).len(), schema().mod_targets.len());
        assert_eq!(options(find("v.palette.paletteId")), ["Marble", "Glaze", "Verdigris", "Ink", "Basalt"]);
        assert!(options(find("fx.filterFreq")).is_empty(), "a slider offers nothing");
        // Every choice on the page has as many options as its range allows.
        for s in &page {
            for g in s.controls.iter().chain(s.toggle.iter()) {
                if g.kind == GeneKind::Choice {
                    assert_eq!(options(g).len(), (g.max - g.min) as usize + 1, "{}", g.id);
                }
            }
        }
    }

    #[test]
    fn a_control_reads_short_under_its_own_section() {
        let page = sections();
        let section = |id: &str| page.iter().find(|s| s.id == id).unwrap();
        let reaction = section("v.reaction");
        let feed = reaction.controls.iter().find(|g| g.id == "v.reaction.feed").unwrap();
        assert_eq!(short_label(reaction, feed), "Feed");
        let lfo = section("lfo.0");
        let rate = lfo.controls.iter().find(|g| g.id == "lfo.0.rate").unwrap();
        assert_eq!(short_label(lfo, rate), "rate");
        // Nothing to strip: the label stands as it is.
        let filter = section("filterOn");
        let freq = filter.controls.iter().find(|g| g.id == "fx.filterFreq").unwrap();
        assert_eq!(short_label(filter, freq), "Filter cutoff");
    }

    #[test]
    fn the_two_tabs_hold_the_sound_and_the_picture() {
        let page = sections();
        let sound: Vec<&str> = page.iter().filter(|s| s.tab == Tab::Sound).map(|s| s.id.as_str()).collect();
        let picture: Vec<&str> =
            page.iter().filter(|s| s.tab == Tab::Picture).map(|s| s.id.as_str()).collect();
        assert!(sound.contains(&"a.fm") && sound.contains(&"filterOn") && sound.contains(&"route.3"));
        assert!(picture.contains(&"v.palette") && picture.contains(&"coupling"));
        assert_eq!(picture.len(), schema().cards.len() + 1);
        assert_eq!(sound.len() + picture.len(), page.len());
    }

    #[test]
    fn an_edit_changes_one_control_and_leaves_the_rest() {
        let point = &crate::state::presets()[0].state;
        let mut edit = Edit::new(point);
        // Every control on the page reads back as itself.
        for s in sections() {
            for g in s.controls.iter().chain(s.toggle.iter()) {
                let v = edit.value(&g.id);
                assert!(v.is_finite(), "{} reads {v}", g.id);
                assert!(
                    v >= g.min - 1e-9 && v <= g.max + 1e-9,
                    "{} is {v}, outside {}..{}",
                    g.id,
                    g.min,
                    g.max
                );
            }
        }
        let before = edit.point();
        edit.set_value("fx.filterFreq", 440.0);
        assert!((edit.value("fx.filterFreq") - 440.0).abs() < 1.0, "{}", edit.value("fx.filterFreq"));
        let after = edit.point();
        assert_ne!(after.audio.fx.filter_freq, before.audio.fx.filter_freq);
        assert_eq!(after.audio.fx.reverb_mix, before.audio.fx.reverb_mix, "one control, one change");
        // The volume and the name are not genes, and survive.
        assert_eq!(after.audio.master_gain, point.audio.master_gain);
        assert_eq!(after.preset_name, point.preset_name);
        edit.set_master_gain(0.3);
        assert_eq!(edit.point().audio.master_gain, 0.3);
        // An id that is not a gene is ignored rather than fatal.
        edit.set_value("nonsense", 1.0);
        assert_eq!(edit.value("nonsense"), 0.0);
    }

    #[test]
    fn a_switch_says_whether_its_controls_do_anything() {
        let point = &crate::state::presets()[0].state;
        let mut edit = Edit::new(point);
        edit.set_value("fx.filterOn", 1.0);
        assert!(edit.active("fx.filterFreq"));
        edit.set_value("fx.filterOn", 0.0);
        assert!(!edit.active("fx.filterFreq"), "an off module's controls do nothing");
        assert!(edit.value("fx.filterFreq") > 0.0, "but their values are still there");
    }

    #[test]
    fn an_edit_is_not_repaired_behind_the_users_back() {
        // Every formula off is what the user chose, even though the search
        // would never leave the point there.
        let point = &crate::state::presets()[0].state;
        let mut edit = Edit::new(point);
        for id in &schema().formula_ids {
            edit.set_value(&format!("a.{id}.enabled"), 0.0);
        }
        assert!(edit.point().enabled_formulas().is_empty());
    }

    #[test]
    fn the_lfo_shapes_line_up_with_the_model() {
        assert_eq!(lfo_shape(0.0), LfoShape::Sine);
        assert_eq!(lfo_shape(4.0), LfoShape::Random);
        assert_eq!(lfo_shape(5.0), LfoShape::Pink);
        assert_eq!(lfo_shape(99.0), LfoShape::Sine, "out of range falls back");
    }
}
