//! The points the user kept, and the file they live in.
//!
//! The file is the console's (`synesthesia-rust`, its `store.rs`): an array
//! of `{ "name": …, "state": { the web app's AppState } }`. That is on
//! purpose — a point file written on a laptop opens on a phone and the other
//! way round (synesthesia-android PLAN.md, decision 7) — and it is why the
//! list is modelled here rather than in each app: the naming rules and the
//! order are part of the file's meaning.
//!
//! No ids and no server (decision 8). A point is kept under the name the user
//! typed, and travels between apps as a file or as a `#s=` token.

use serde::{Deserialize, Serialize};
use syn_core::state::AppState;

/// A point the user kept, under the name they gave it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedPoint {
    pub name: String,
    pub state: AppState,
}

/// The kept points, in the order they were kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Points {
    list: Vec<NamedPoint>,
}

impl Points {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the points file. An error names what is wrong with it, so an app
    /// can say so instead of quietly starting with none.
    pub fn parse(json: &str) -> Result<Self, String> {
        let text = json.trim();
        if text.is_empty() {
            return Ok(Self::new());
        }
        serde_json::from_str(text).map(|list| Points { list }).map_err(|e| e.to_string())
    }

    /// The file to write, formatted as the console writes it (a person may
    /// well open it in an editor).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.list).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&NamedPoint> {
        self.list.get(index)
    }

    pub fn names(&self) -> Vec<&str> {
        self.list.iter().map(|p| p.name.as_str()).collect()
    }

    /// Keeps `state` under `name`, replacing a point of the same name rather
    /// than growing a second one. Returns where it ended up.
    pub fn keep(&mut self, name: &str, state: &AppState) -> usize {
        let mut state = state.clone();
        // The point carries its own name, so the file and a token agree about
        // what it is called.
        state.preset_name = Some(name.to_string());
        let entry = NamedPoint { name: name.to_string(), state };
        match self.list.iter().position(|p| p.name == name) {
            Some(i) => {
                self.list[i] = entry;
                i
            }
            None => {
                self.list.push(entry);
                self.list.len() - 1
            }
        }
    }

    /// Forgets the point at `index`; the point itself keeps playing.
    pub fn remove(&mut self, index: usize) -> Option<NamedPoint> {
        (index < self.list.len()).then(|| self.list.remove(index))
    }

    /// The name to offer when saving.
    ///
    /// Re-saving one of your own points offers the same name, which overwrites
    /// it; anything else — including a built-in point's name — gets a fresh
    /// "Point N", so that a saved copy is never indistinguishable from the
    /// built-in it came from (`suggestPointName`: users read that as "it
    /// didn't save").
    pub fn suggest_name(&self, current: Option<&str>) -> String {
        match current {
            Some(name) if self.list.iter().any(|p| p.name == name) => name.to_string(),
            _ => format!("Point {}", self.next_number()),
        }
    }

    /// The next free number for the automatic "Point N".
    fn next_number(&self) -> u32 {
        let highest = self
            .list
            .iter()
            .filter_map(|p| p.name.strip_prefix("Point ").and_then(|n| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        highest + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn_core::state::presets;

    fn point(i: usize) -> AppState {
        presets()[i].state.clone()
    }

    /// A point without the name it carries, so two can be compared as points.
    fn nameless(state: &AppState) -> AppState {
        AppState { preset_name: None, ..state.clone() }
    }

    #[test]
    fn a_kept_point_comes_back_from_the_file_unchanged() {
        let mut points = Points::new();
        points.keep("Dawn", &point(3));
        points.keep("Dusk", &point(8));
        let file = points.to_json();

        let back = Points::parse(&file).expect("parses");
        assert_eq!(back.names(), ["Dawn", "Dusk"]);
        // The point itself is the web app's JSON, down to its name.
        let kept = back.get(0).expect("the first point");
        assert_eq!(kept.state.preset_name.as_deref(), Some("Dawn"));
        assert_eq!(nameless(&kept.state), nameless(&point(3)));
        assert!(file.contains("\"name\": \"Dawn\""), "{file}");
    }

    #[test]
    fn keeping_a_name_twice_replaces_it() {
        let mut points = Points::new();
        assert_eq!(points.keep("Mine", &point(1)), 0);
        assert_eq!(points.keep("Other", &point(2)), 1);
        assert_eq!(points.keep("Mine", &point(5)), 0, "it goes back where it was");
        assert_eq!(points.len(), 2);
        assert_eq!(nameless(&points.get(0).unwrap().state), nameless(&point(5)), "and it is the new point");
    }

    #[test]
    fn a_forgotten_point_leaves_the_list() {
        let mut points = Points::new();
        points.keep("One", &point(0));
        points.keep("Two", &point(1));
        assert_eq!(points.remove(0).map(|p| p.name), Some("One".to_string()));
        assert_eq!(points.names(), ["Two"]);
        assert!(points.remove(9).is_none());
        assert!(points.remove(0).is_some());
        assert!(points.is_empty());
    }

    #[test]
    fn the_name_offered_overwrites_your_own_and_never_a_built_in() {
        let mut points = Points::new();
        assert_eq!(points.suggest_name(None), "Point 1");
        assert_eq!(points.suggest_name(Some("Fractal garden")), "Point 1", "not the built-in's name");

        points.keep("Point 1", &point(0));
        assert_eq!(points.suggest_name(None), "Point 2");
        assert_eq!(points.suggest_name(Some("Point 1")), "Point 1", "re-saving your own overwrites it");

        points.keep("Point 7", &point(0));
        assert_eq!(points.suggest_name(None), "Point 8", "after the highest one");
        points.keep("Dawn", &point(0));
        assert_eq!(points.suggest_name(Some("Dawn")), "Dawn");
    }

    #[test]
    fn an_empty_or_broken_file_is_told_apart() {
        assert!(Points::parse("").expect("nothing is no points").is_empty());
        assert!(Points::parse("   ").expect("whitespace too").is_empty());
        assert!(Points::parse("[]").expect("an empty list").is_empty());
        let broken = Points::parse("{oh dear");
        assert!(broken.is_err(), "a broken file is not silently empty");
    }
}
