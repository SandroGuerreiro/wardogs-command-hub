pub mod coords;
pub mod entity;
pub mod header;
pub mod places;

pub use coords::{find_coords, Coord, CoordMatch};
pub use entity::{classify_entity, Entity};
pub use header::{parse_header, Channel, Header};
pub use places::{PlaceIndex, PlaceMatch};

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocationKind {
    Coord,
    Named,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub kind: LocationKind,
    pub coord: Coord,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: String,
    pub at_ms: u64,
    pub channel: Channel,
    pub clan: Option<String>,
    pub name: String,
    pub body: String,
    pub raw: String,
    pub location: Option<Location>,
    pub entity: Entity,
}

/// Derives an id from `raw` and `at_ms` so a line re-seen after ring
/// eviction (same text, later timestamp) gets a distinct id rather than
/// colliding with the earlier message. These ids are in-session only:
/// `DefaultHasher` is not guaranteed stable across Rust toolchains or runs,
/// so they must never be persisted or compared across process restarts.
fn stable_id(raw: &str, at_ms: u64) -> String {
    let mut h = DefaultHasher::new();
    raw.hash(&mut h);
    at_ms.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn locate(body: &str, places: &PlaceIndex) -> (Option<Location>, String) {
    if let Some(m) = find_coords(body) {
        let loc = Location {
            kind: LocationKind::Coord,
            coord: m.coord,
            label: None,
        };
        return (Some(loc), m.rest);
    }
    if let Some(p) = places.find(body) {
        let loc = Location {
            kind: LocationKind::Named,
            coord: p.coord,
            label: Some(p.name),
        };
        return (Some(loc), body.to_string());
    }
    (None, body.to_string())
}

/// Turn one de-duplicated chat line into a `Message`. Never fails: lines
/// without a header become `Channel::Unknown` with the whole line as body.
pub fn parse_message(raw: &str, places: &PlaceIndex, at_ms: u64) -> Message {
    let header = parse_header(raw).unwrap_or(Header {
        channel: Channel::Unknown,
        clan: None,
        name: String::new(),
        body: raw.trim().to_string(),
    });
    let (location, body) = locate(&header.body, places);
    let entity = classify_entity(&body);
    Message {
        id: stable_id(raw, at_ms),
        at_ms,
        channel: header.channel,
        clan: header.clan,
        name: header.name,
        body,
        raw: raw.to_string(),
        location,
        entity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn places() -> PlaceIndex {
        let mut p = BTreeMap::new();
        p.insert("tower 5".to_string(), Coord { x: 41.2, y: 99.3 });
        PlaceIndex::new(&p, &BTreeMap::new())
    }

    #[test]
    fn coordinate_message() {
        let m = parse_message(
            "[TEAM] LeftWild: x90.97, y101.30 need ammo here for tower 1",
            &places(),
            1000,
        );
        assert_eq!(m.channel, Channel::Team);
        assert_eq!(m.name, "LeftWild");
        let loc = m.location.unwrap();
        assert_eq!(loc.kind, LocationKind::Coord);
        assert_eq!(
            loc.coord,
            Coord {
                x: 90.97,
                y: 101.30
            }
        );
        assert_eq!(m.body, "need ammo here for tower 1");
        assert_eq!(m.at_ms, 1000);
    }

    #[test]
    fn named_message() {
        let m = parse_message("[TEAM] Fl4sh: one is in tower 5", &places(), 0);
        let loc = m.location.unwrap();
        assert_eq!(loc.kind, LocationKind::Named);
        assert_eq!(loc.label.as_deref(), Some("tower 5"));
        assert_eq!(m.entity, Entity::Infantry);
    }

    #[test]
    fn coordinate_beats_named() {
        let m = parse_message("[TEAM] a: x1, y2 tower 5", &places(), 0);
        assert_eq!(m.location.unwrap().kind, LocationKind::Coord);
    }

    #[test]
    fn unparseable_line_is_kept_as_unknown() {
        let m = parse_message("garbage ocr line", &places(), 0);
        assert_eq!(m.channel, Channel::Unknown);
        assert_eq!(m.name, "");
        assert_eq!(m.body, "garbage ocr line");
        assert!(m.location.is_none());
    }

    #[test]
    fn id_is_stable_for_same_raw_and_at_ms() {
        let a = parse_message("[TEAM] a: b", &places(), 1);
        let b = parse_message("[TEAM] a: b", &places(), 1);
        assert_eq!(a.id, b.id);
        assert_ne!(a.id, parse_message("[TEAM] a: c", &places(), 1).id);
    }

    #[test]
    fn id_differs_for_same_raw_different_at_ms() {
        let a = parse_message("[TEAM] a: b", &places(), 1);
        let b = parse_message("[TEAM] a: b", &places(), 2);
        assert_ne!(a.id, b.id);
    }
}
