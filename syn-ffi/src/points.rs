//! Points and tokens through the FFI.
//!
//! A point is kept locally, under the name the user typed, in the file the
//! console writes — so a point file moves between a laptop and a phone as it
//! is (synesthesia-android PLAN.md, decision 7) — and it travels between the
//! apps as the web app's `#s=` token, by copy and paste. There are no ids and
//! no server (decision 8): what this crate offers is the list's rules, the
//! token, and what a link turns out to point at.
//!
//! The app owns the files. This owns what is in them.

use std::sync::{Arc, Mutex, MutexGuard};

use syn_core::share;
use syn_session::points::Points;

use crate::{parse_point, CoreError};

/// The points the user kept, in the order they were kept.
#[derive(uniffi::Object)]
pub struct PointList {
    inner: Mutex<Points>,
}

#[uniffi::export]
impl PointList {
    /// An empty list — a device with no points file yet.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(PointList { inner: Mutex::new(Points::new()) })
    }

    /// The list a points file holds. Empty text is simply no points; text
    /// that is not a points file is an error, so an app can say so instead of
    /// quietly starting with none and overwriting it.
    #[uniffi::constructor]
    pub fn parse(json: String) -> Result<Arc<Self>, CoreError> {
        let points = Points::parse(&json).map_err(|reason| CoreError::BadPointsFile { reason })?;
        Ok(Arc::new(PointList { inner: Mutex::new(points) }))
    }

    pub fn names(&self) -> Vec<String> {
        self.locked().names().into_iter().map(str::to_string).collect()
    }

    pub fn count(&self) -> u32 {
        self.locked().len() as u32
    }

    /// The point at `index` as the web app's JSON, ready for
    /// `Session.load`.
    pub fn point_json(&self, index: u32) -> Option<String> {
        let points = self.locked();
        let kept = points.get(index as usize)?;
        serde_json::to_string(&kept.state).ok()
    }

    pub fn name_at(&self, index: u32) -> Option<String> {
        self.locked().get(index as usize).map(|p| p.name.clone())
    }

    /// Keeps a point under `name`, replacing one of the same name rather than
    /// growing a second. Returns where it ended up; write
    /// [`PointList::to_json`] to the file afterwards.
    pub fn keep(&self, name: String, point_json: String) -> Result<u32, CoreError> {
        let state = parse_point(&point_json)?;
        Ok(self.locked().keep(&name, &state) as u32)
    }

    /// Forgets the point at `index` — the point itself keeps playing.
    pub fn forget(&self, index: u32) -> bool {
        self.locked().remove(index as usize).is_some()
    }

    /// The name to offer when saving: the same name again for one of the
    /// user's own points (which overwrites it), and a fresh "Point N" for
    /// anything else, including a built-in point's name.
    pub fn suggest_name(&self, current: Option<String>) -> String {
        self.locked().suggest_name(current.as_deref())
    }

    /// The file to write, formatted as the console writes it.
    pub fn to_json(&self) -> String {
        self.locked().to_json()
    }
}

impl PointList {
    fn locked(&self) -> MutexGuard<'_, Points> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The point as the web app's `#s=` token — the whole of how a point travels
/// between the apps.
#[uniffi::export]
pub fn point_token(point_json: String) -> Result<String, CoreError> {
    Ok(share::encode_token(&parse_point(&point_json)?))
}

/// A point from a token, or from the whole link it was pasted inside.
#[uniffi::export]
pub fn point_from_token(text: String) -> Result<String, CoreError> {
    let state = share::decode_token(&text)
        .ok_or_else(|| CoreError::InvalidPoint { reason: "not a point token".into() })?;
    serde_json::to_string(&state).map_err(|e| CoreError::InvalidPoint { reason: e.to_string() })
}

/// What a link turns out to point at.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkPoint {
    /// A point carried whole, in a `#s=` token.
    Point { point_json: String },
    /// One of the built-in points, by its number.
    Preset { index: u32 },
    /// A point kept on the web app's server. There is no network here
    /// (PLAN.md decision 8), so an app says so rather than opening something
    /// else and looking broken.
    NeedsTheWebApp { id: String },
    /// Nothing in the link names a point.
    Nothing,
}

/// What a link opens — for an intent that handed the app the web app's URL,
/// in the same order of priority the web app reads it in.
#[uniffi::export]
pub fn point_from_link(url: String) -> LinkPoint {
    match share::parse_launch(&url) {
        share::Launch::Point(state) => match serde_json::to_string(&*state) {
            Ok(point_json) => LinkPoint::Point { point_json },
            Err(_) => LinkPoint::Nothing,
        },
        share::Launch::Preset(index) => LinkPoint::Preset { index: index as u32 },
        share::Launch::Stored(id) => LinkPoint::NeedsTheWebApp { id },
        share::Launch::Nothing => LinkPoint::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{preset_state_json, presets};

    #[test]
    fn a_point_is_kept_under_a_name_and_comes_back_from_the_file() {
        let list = PointList::new();
        assert_eq!(list.count(), 0);
        assert_eq!(list.suggest_name(None), "Point 1");
        assert_eq!(list.suggest_name(Some("Fractal garden".into())), "Point 1", "never a built-in's name");

        assert_eq!(list.keep("Dawn".into(), preset_state_json(3).unwrap()).unwrap(), 0);
        assert_eq!(list.keep("Dusk".into(), preset_state_json(8).unwrap()).unwrap(), 1);
        assert_eq!(list.names(), ["Dawn", "Dusk"]);
        assert_eq!(list.suggest_name(Some("Dawn".into())), "Dawn", "re-saving overwrites");

        let file = list.to_json();
        let back = PointList::parse(file).expect("the file parses");
        assert_eq!(back.names(), ["Dawn", "Dusk"]);
        assert_eq!(back.name_at(1).as_deref(), Some("Dusk"));
        // What comes back is a point a session can load and a player can play.
        let point = back.point_json(0).expect("the first point");
        assert!(point.contains("\"presetName\":\"Dawn\""));
        assert!(crate::SoundPlayer::new(22050, point).is_ok());

        assert!(back.forget(0));
        assert_eq!(back.names(), ["Dusk"]);
        assert!(!back.forget(5));
        assert!(back.point_json(5).is_none() && back.name_at(5).is_none());
    }

    #[test]
    fn an_unreadable_points_file_is_an_error_not_an_empty_list() {
        assert_eq!(PointList::parse(String::new()).unwrap().count(), 0);
        assert_eq!(PointList::parse("[]".into()).unwrap().count(), 0);
        assert!(matches!(PointList::parse("{oh dear".into()), Err(CoreError::BadPointsFile { .. })));
        let list = PointList::new();
        assert!(matches!(list.keep("x".into(), "{}".into()), Err(CoreError::InvalidPoint { .. })));
    }

    #[test]
    fn a_point_travels_as_a_token_and_comes_back_from_one() {
        let json = preset_state_json(5).unwrap();
        let token = point_token(json.clone()).expect("a token");
        assert!(!token.is_empty() && !token.contains('#'));
        assert_eq!(point_from_token(token.clone()).unwrap(), json);
        // The link it would be pasted from works too, spaces and all.
        let link = format!("  https://dmitryweiner.github.io/synesthesia/#s={token}  ");
        assert_eq!(point_from_token(link).unwrap(), json);
        assert!(matches!(point_from_token("nope".into()), Err(CoreError::InvalidPoint { .. })));
        assert!(matches!(point_token("{}".into()), Err(CoreError::InvalidPoint { .. })));
    }

    #[test]
    fn a_link_says_what_it_opens() {
        let json = preset_state_json(2).unwrap();
        let token = point_token(json.clone()).unwrap();
        let site = "https://dmitryweiner.github.io/synesthesia/";

        assert_eq!(point_from_link(format!("{site}#s={token}")), LinkPoint::Point { point_json: json });
        assert_eq!(point_from_link(format!("{site}?preset=4")), LinkPoint::Preset { index: 4 });
        assert_eq!(
            point_from_link(format!("{site}?presetId=aB3dE6gH9j")),
            LinkPoint::NeedsTheWebApp { id: "aB3dE6gH9j".into() },
        );
        assert_eq!(point_from_link(site.into()), LinkPoint::Nothing);
        // A preset number out of range is the app's to notice, not the link's.
        assert_eq!(point_from_link(format!("{site}?preset=99")), LinkPoint::Preset { index: 99 });
        assert!(presets().len() < 99, "which is past the end of the built-in list");
    }
}
