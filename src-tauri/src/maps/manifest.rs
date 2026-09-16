use super::calibration::Calibration;
use crate::parser::{Coord, PlaceIndex};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("map.json is not valid JSON: {0}")]
    Json(String),
    #[error("map.json is invalid: {0}")]
    Invalid(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapManifest {
    pub id: String,
    pub name: String,
    pub source_size: [u32; 2],
    #[serde(default)]
    pub calibration: Option<Calibration>,
    #[serde(default)]
    pub places: BTreeMap<String, Coord>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl MapManifest {
    pub fn from_json(text: &str) -> Result<Self, ManifestError> {
        let m: MapManifest =
            serde_json::from_str(text).map_err(|e| ManifestError::Json(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        let inv = |s: String| Err(ManifestError::Invalid(s));
        if !valid_id(&self.id) {
            return inv(format!(
                "id '{}' must be lowercase letters, digits or '-'",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return inv("name is empty".into());
        }
        if self.source_size.contains(&0) {
            return inv("sourceSize must be positive".into());
        }
        if let Some(c) = &self.calibration {
            c.validate()
                .map_err(|e| ManifestError::Invalid(e.to_string()))?;
        }
        for (alias, target) in &self.aliases {
            if !self.places.contains_key(target) {
                return inv(format!(
                    "alias '{alias}' points to unknown place '{target}'"
                ));
            }
        }
        Ok(())
    }

    pub fn place_index(&self) -> PlaceIndex {
        PlaceIndex::new(&self.places, &self.aliases)
    }
}

#[derive(Debug, Default)]
pub struct MapRegistry {
    pub maps: BTreeMap<String, MapManifest>,
    pub errors: BTreeMap<String, String>,
}

impl MapRegistry {
    /// Load every `<dir>/<folder>/map.json`. Folders that fail land in
    /// `errors` keyed by folder name; the registry itself only fails if
    /// `dir` cannot be read.
    pub fn load_dir(dir: &Path) -> Result<Self, ManifestError> {
        let mut reg = MapRegistry::default();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            let folder = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            match load_one(&path) {
                Ok(m) => {
                    reg.maps.insert(m.id.clone(), m);
                }
                Err(e) => {
                    reg.errors.insert(folder, e.to_string());
                }
            }
        }
        Ok(reg)
    }

    pub fn get(&self, id: &str) -> Option<&MapManifest> {
        self.maps.get(id)
    }
}

fn load_one(folder: &Path) -> Result<MapManifest, ManifestError> {
    let text = std::fs::read_to_string(folder.join("map.json"))?;
    MapManifest::from_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
      "id": "bakurani", "name": "Bakurani", "sourceSize": [16384, 16384],
      "calibration": { "a": { "game": {"x": 0, "y": 0}, "px": {"x": 0, "y": 0} },
                       "b": { "game": {"x": 100, "y": 100}, "px": {"x": 16384, "y": 16384} } },
      "places": { "tower 5": {"x": 41.2, "y": 99.3} },
      "aliases": { "t5": "tower 5" }
    }"#;

    #[test]
    fn parses_full_manifest() {
        let m = MapManifest::from_json(GOOD).unwrap();
        assert_eq!(m.id, "bakurani");
        assert!(m.calibration.is_some());
        assert_eq!(m.places.len(), 1);
        assert_eq!(m.place_index().find("go t5").unwrap().name, "tower 5");
    }

    #[test]
    fn calibration_may_be_null_and_places_optional() {
        let m = MapManifest::from_json(
            r#"{"id":"x","name":"X","sourceSize":[10,10],"calibration":null}"#,
        )
        .unwrap();
        assert!(m.calibration.is_none());
        assert!(m.places.is_empty());
    }

    #[test]
    fn rejects_bad_id_and_size() {
        assert!(matches!(
            MapManifest::from_json(r#"{"id":"","name":"X","sourceSize":[10,10]}"#),
            Err(ManifestError::Invalid(_))
        ));
        assert!(matches!(
            MapManifest::from_json(r#"{"id":"Bad Id","name":"X","sourceSize":[10,10]}"#),
            Err(ManifestError::Invalid(_))
        ));
        assert!(matches!(
            MapManifest::from_json(r#"{"id":"x","name":"X","sourceSize":[0,10]}"#),
            Err(ManifestError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_alias_to_unknown_place_and_degenerate_calibration() {
        let bad_alias = r#"{"id":"x","name":"X","sourceSize":[10,10],"aliases":{"a":"nowhere"}}"#;
        assert!(matches!(
            MapManifest::from_json(bad_alias),
            Err(ManifestError::Invalid(_))
        ));
        let bad_cal = r#"{"id":"x","name":"X","sourceSize":[10,10],
          "calibration":{"a":{"game":{"x":0,"y":0},"px":{"x":0,"y":0}},"b":{"game":{"x":0,"y":0},"px":{"x":0,"y":0}}}}"#;
        assert!(matches!(
            MapManifest::from_json(bad_cal),
            Err(ManifestError::Invalid(_))
        ));
    }

    #[test]
    fn malformed_json_is_json_error() {
        assert!(matches!(
            MapManifest::from_json("{"),
            Err(ManifestError::Json(_))
        ));
    }

    #[test]
    fn registry_loads_repo_maps_and_reports_bad_ones() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../maps");
        let reg = MapRegistry::load_dir(&dir).unwrap();
        for id in ["bakurani", "ozeti", "zestafona"] {
            assert!(reg.get(id).is_some(), "missing {id}");
        }
        assert!(reg.errors.is_empty(), "{:?}", reg.errors);
    }

    #[test]
    fn registry_reports_io_error_when_map_json_missing() {
        let tmp = std::env::temp_dir().join(format!("hub-maps-noio-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("empty")).unwrap();
        let reg = MapRegistry::load_dir(&tmp).unwrap();
        assert!(reg.maps.is_empty());
        let err = reg.errors.get("empty").expect("empty folder should error");
        assert!(err.contains("io error"), "unexpected error: {err}");
        std::fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn registry_collects_errors_without_failing() {
        let tmp = std::env::temp_dir().join(format!("hub-maps-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("broken")).unwrap();
        std::fs::write(tmp.join("broken/map.json"), "{").unwrap();
        let reg = MapRegistry::load_dir(&tmp).unwrap();
        assert!(reg.maps.is_empty());
        assert!(reg.errors.contains_key("broken"));
        std::fs::remove_dir_all(tmp).unwrap();
    }
}
