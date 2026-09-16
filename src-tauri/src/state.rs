use crate::maps::MapManifest;
use crate::parser::{Coord, Entity, Message};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pin {
    pub message_id: String,
    pub px: Coord,
    pub game: Coord,
    pub entity: Entity,
    pub at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub messages: Vec<Message>,
    pub pins: Vec<Pin>,
    pub map_id: Option<String>,
}

fn pin_for(msg: &Message, map: Option<&MapManifest>, ttl_ms: u64) -> Option<Pin> {
    let loc = msg.location.as_ref()?;
    let cal = map?.calibration?;
    Some(Pin {
        message_id: msg.id.clone(),
        px: cal.to_pixel(loc.coord),
        game: loc.coord,
        entity: msg.entity,
        at_ms: msg.at_ms,
        expires_at_ms: msg.at_ms.saturating_add(ttl_ms),
    })
}

/// Append a message (and its pin when the map is calibrated). Returns a new
/// snapshot; `prev` is untouched. Messages are capped at `max_messages`,
/// oldest dropped.
pub fn apply(
    prev: &Snapshot,
    msg: Message,
    map: Option<&MapManifest>,
    ttl_ms: u64,
    max_messages: usize,
) -> Snapshot {
    let pin = pin_for(&msg, map, ttl_ms);
    let messages: Vec<Message> = prev
        .messages
        .iter()
        .cloned()
        .chain(std::iter::once(msg))
        .collect();
    let skip = messages.len().saturating_sub(max_messages.max(1));
    Snapshot {
        messages: messages.into_iter().skip(skip).collect(),
        pins: prev.pins.iter().cloned().chain(pin).collect(),
        map_id: prev.map_id.clone(),
    }
}

/// Drop pins whose TTL has passed. Messages are kept.
pub fn expire(prev: &Snapshot, now_ms: u64) -> Snapshot {
    Snapshot {
        messages: prev.messages.clone(),
        pins: prev
            .pins
            .iter()
            .filter(|p| p.expires_at_ms > now_ms)
            .cloned()
            .collect(),
        map_id: prev.map_id.clone(),
    }
}

/// Switch map. Pins belong to the old map's pixel space, so they are cleared.
pub fn with_map(prev: &Snapshot, map_id: Option<String>) -> Snapshot {
    Snapshot {
        messages: prev.messages.clone(),
        pins: Vec::new(),
        map_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::{CalPoint, Calibration, MapManifest};
    use crate::parser::{parse_message, Coord, PlaceIndex};

    fn map(calibrated: bool) -> MapManifest {
        let cal = Calibration {
            a: CalPoint {
                game: Coord { x: 0.0, y: 0.0 },
                px: Coord { x: 0.0, y: 0.0 },
            },
            b: CalPoint {
                game: Coord { x: 100.0, y: 100.0 },
                px: Coord {
                    x: 1000.0,
                    y: 1000.0,
                },
            },
        };
        MapManifest {
            id: "t".into(),
            name: "T".into(),
            source_size: [1000, 1000],
            calibration: calibrated.then_some(cal),
            places: Default::default(),
            aliases: Default::default(),
        }
    }

    fn msg(raw: &str, at: u64) -> Message {
        parse_message(raw, &PlaceIndex::empty(), at)
    }

    #[test]
    fn located_message_on_calibrated_map_makes_pin() {
        let s = apply(
            &Snapshot::default(),
            msg("[TEAM] a: x50, y25 tank", 1000),
            Some(&map(true)),
            60_000,
            100,
        );
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.pins.len(), 1);
        assert_eq!(s.pins[0].px, Coord { x: 500.0, y: 250.0 });
        assert_eq!(s.pins[0].expires_at_ms, 61_000);
        assert_eq!(s.pins[0].entity, Entity::Vehicle);
    }

    #[test]
    fn uncalibrated_or_no_map_keeps_message_but_no_pin() {
        let s = apply(
            &Snapshot::default(),
            msg("[TEAM] a: x50, y25", 0),
            Some(&map(false)),
            60_000,
            100,
        );
        assert_eq!(s.messages.len(), 1);
        assert!(s.pins.is_empty());
        let s = apply(
            &Snapshot::default(),
            msg("[TEAM] a: x50, y25", 0),
            None,
            60_000,
            100,
        );
        assert!(s.pins.is_empty());
    }

    #[test]
    fn unlocated_message_makes_no_pin() {
        let s = apply(
            &Snapshot::default(),
            msg("[TEAM] a: hello", 0),
            Some(&map(true)),
            60_000,
            100,
        );
        assert!(s.pins.is_empty());
    }

    #[test]
    fn apply_does_not_mutate_previous() {
        let first = apply(
            &Snapshot::default(),
            msg("[TEAM] a: x1, y1", 0),
            Some(&map(true)),
            1000,
            100,
        );
        let _second = apply(
            &first,
            msg("[TEAM] a: x2, y2", 0),
            Some(&map(true)),
            1000,
            100,
        );
        assert_eq!(first.messages.len(), 1);
        assert_eq!(first.pins.len(), 1);
    }

    #[test]
    fn messages_capped_oldest_dropped() {
        let mut s = Snapshot::default();
        for i in 0..5 {
            s = apply(&s, msg(&format!("[TEAM] a: {i}"), i), None, 1000, 3);
        }
        assert_eq!(s.messages.len(), 3);
        assert_eq!(s.messages[0].body, "2");
    }

    #[test]
    fn expire_drops_old_pins_only() {
        let s = apply(
            &Snapshot::default(),
            msg("[TEAM] a: x1, y1", 0),
            Some(&map(true)),
            1000,
            100,
        );
        let s = apply(
            &s,
            msg("[TEAM] a: x2, y2", 5000),
            Some(&map(true)),
            1000,
            100,
        );
        let e = expire(&s, 3000);
        assert_eq!(e.pins.len(), 1);
        assert_eq!(e.messages.len(), 2);
    }

    #[test]
    fn with_map_clears_pins() {
        let s = apply(
            &Snapshot::default(),
            msg("[TEAM] a: x1, y1", 0),
            Some(&map(true)),
            1000,
            100,
        );
        let s = with_map(&s, Some("other".into()));
        assert!(s.pins.is_empty());
        assert_eq!(s.map_id.as_deref(), Some("other"));
    }
}
