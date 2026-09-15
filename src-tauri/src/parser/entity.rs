#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Entity {
    Vehicle,
    Structure,
    Infantry,
    Other,
}

const VEHICLE: &[&str] = &[
    "tank",
    "apc",
    "ifv",
    "heli",
    "helicopter",
    "chopper",
    "truck",
    "jeep",
    "humvee",
    "car",
    "vehicle",
    "vic",
    "bike",
    "boat",
    "mrap",
    "btr",
    "bmp",
    "sph",
    "artillery truck",
];
const STRUCTURE: &[&str] = &[
    "bunker",
    "fob",
    "hab",
    "mortar",
    "turret",
    "aa",
    "wall",
    "walls",
    "gate",
    "radio",
    "outpost",
    "emplacement",
    "sandbag",
    "hesco",
    "spawn",
    "structure",
    "building",
];
const INFANTRY: &[&str] = &[
    "sniper", "guy", "guys", "man", "men", "inf", "infantry", "squad", "enemy", "enemys",
    "enemies", "one", "two", "three", "pushing", "flanking", "camping", "player", "players",
];

fn has_keyword(words: &[String], list: &[&str]) -> bool {
    list.iter().any(|k| {
        if k.contains(' ') {
            let k_tokens: Vec<&str> = k.split_whitespace().collect();
            words
                .windows(k_tokens.len())
                .any(|w| w.iter().map(|s| s.as_str()).collect::<Vec<_>>() == k_tokens)
        } else {
            words.iter().any(|w| w == k)
        }
    })
}

/// Classify what a chat body is reporting. Checked in priority order:
/// vehicle, then infantry, then structure. Structure words like "tower" are
/// only structural when no infantry word accompanies them, since "one is in
/// tower 5" reports infantry at a tower.
pub fn classify_entity(body: &str) -> Entity {
    let words: Vec<String> = body
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    if has_keyword(&words, VEHICLE) {
        return Entity::Vehicle;
    }
    let infantry = has_keyword(&words, INFANTRY);
    let structure = has_keyword(&words, STRUCTURE);
    match (infantry, structure) {
        (true, _) => Entity::Infantry,
        (false, true) => Entity::Structure,
        (false, false) => Entity::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicles() {
        for s in [
            "enemy tank on the ridge",
            "APC coming",
            "heli over tower 2",
            "truck at bridge",
            "jeep",
        ] {
            assert_eq!(classify_entity(s), Entity::Vehicle, "{s}");
        }
    }

    #[test]
    fn structures() {
        for s in [
            "they built a bunker",
            "fob built here",
            "mortar pit at x",
            "AA turret",
            "walls going up",
        ] {
            assert_eq!(classify_entity(s), Entity::Structure, "{s}");
        }
    }

    #[test]
    fn infantry() {
        for s in [
            "one is in tower 5",
            "sniper on hill",
            "3 guys pushing",
            "squad flanking left",
            "enemys in there",
        ] {
            assert_eq!(classify_entity(s), Entity::Infantry, "{s}");
        }
    }

    #[test]
    fn other() {
        assert_eq!(
            classify_entity("supplies delivered to tower 5"),
            Entity::Other
        );
        assert_eq!(classify_entity("need ammo here"), Entity::Other);
        assert_eq!(classify_entity(""), Entity::Other);
    }

    #[test]
    fn first_category_by_priority_wins() {
        // "tank" (vehicle) beats "guys" (infantry) because vehicle keywords are checked first
        assert_eq!(classify_entity("tank with guys around"), Entity::Vehicle);
    }

    #[test]
    fn multi_word_keyword_matching() {
        // "artillery" alone is not a keyword, so it's Other
        assert_eq!(classify_entity("artillery"), Entity::Other);
        // "artillery truck" is an exact multi-word match, so it's Vehicle
        assert_eq!(classify_entity("artillery truck"), Entity::Vehicle);
        // "artillery trucks" doesn't match "artillery truck" (trucks != truck), so it's Other
        assert_eq!(classify_entity("artillery trucks"), Entity::Other);
    }
}
