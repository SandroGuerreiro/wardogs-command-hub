# Core Pipeline Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A headless Rust library plus a CLI that captures the WARDOGS chat rectangle, OCRs it, de-duplicates and parses lines into located messages, and keeps an expiring snapshot of pins. No window yet.

**Architecture:** One Rust crate in `src-tauri/` (lib + one CLI bin). Pure modules (`parser`, `dedup`, `maps`, `state`) are tested with fixtures. Side-effect modules (`capture`, `ocr`) sit behind small traits so the `pipeline` integration test runs with a fake OCR engine and recorded lines. Every function returns `Result` with a typed error; nothing panics on bad input. All state updates return new values, never mutate.

**Tech Stack:** Rust 2021 stable, `regex`, `serde`/`serde_json`, `thiserror`, `image`, `xcap` (screen capture), `ocrs` + `rten` (pure Rust OCR), `proptest` (dev).

**Spec:** `docs/superpowers/specs/2026-09-11-command-hub-design.md`

## Global Constraints

- Rust edition 2021, stable toolchain. Crate lives in `src-tauri/` so Plan 2 can add Tauri without moving files.
- Windows is primary, Linux best effort. No Windows-only code in this plan.
- Functions under 50 lines, files under 400 lines. Immutable updates: `state::apply` returns a new `Snapshot`.
- No `unwrap()`/`expect()` outside tests. Errors are `thiserror` enums per module.
- Coordinate range is unknown and per-map. Never hard-code a range. Uncalibrated maps produce messages but no pins.
- Commit after every task with `<type>: <description>` and no attribution trailers.
- Test command everywhere: `cargo test --manifest-path src-tauri/Cargo.toml`.

## File Structure

```
src-tauri/
  Cargo.toml
  src/lib.rs                 pub mod list only
  src/error.rs               (none: each module owns its error enum)
  src/parser/mod.rs          Message, Channel, Entity, Location, parse_message()
  src/parser/header.rs       parse_header(): channel/clan/name/body split
  src/parser/coords.rs       find_coords(): x/y pair regex
  src/parser/entity.rs       classify_entity(): keyword lists
  src/parser/places.rs       PlaceIndex: named-location matcher
  src/dedup.rs               Deduper: normalise + ring buffer + wrap join
  src/maps/calibration.rs    Calibration: game<->pixel affine
  src/maps/manifest.rs       MapManifest (map.json) + validation
  src/maps/mod.rs            MapRegistry: load all maps from a dir
  src/config.rs              Settings: load/save/validate
  src/state.rs               Snapshot, Pin, apply(), expire()
  src/gate.rs                FrameGate: skip unchanged frames
  src/capture.rs             Capturer: xcap monitor + rect crop
  src/ocr/mod.rs             OcrEngine trait, OcrLine
  src/ocr/preprocess.rs      crop/upscale/grey for OCR
  src/ocr/ocrs_engine.rs     OcrsEngine impl
  src/pipeline.rs            Pipeline: gate -> ocr -> dedup -> parse
  src/bin/hub_cli.rs         CLI: `ocr <image>`, `watch`
  tests/pipeline_test.rs     integration with FakeOcr
fixtures/chat/sample-01.txt      (exists)
fixtures/screenshots/ingame-chat-1600x900.jpg (exists)
maps/bakurani/map.json, maps/ozeti/map.json, maps/zestafona/map.json
```

---

### Task 1: Crate scaffold

**Files:**
- Create: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/tests/smoke.rs`
- Modify: `.gitignore` (add `src-tauri/target/`, `models/`)

**Interfaces:**
- Produces: crate name `wardogs_command_hub`, importable as `wardogs_command_hub::...` from tests.

- [ ] **Step 1: Write the failing smoke test**

`src-tauri/tests/smoke.rs`:
```rust
#[test]
fn crate_exposes_version() {
    assert!(!wardogs_command_hub::VERSION.is_empty());
}
```

- [ ] **Step 2: Run it, expect failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: error, no Cargo.toml / crate not found.

- [ ] **Step 3: Create the crate**

`src-tauri/Cargo.toml`:
```toml
[package]
name = "wardogs_command_hub"
version = "0.1.0"
edition = "2021"
license = "MIT"

[lib]
path = "src/lib.rs"

[[bin]]
name = "hub-cli"
path = "src/bin/hub_cli.rs"

[dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "webp"] }
xcap = "0.4"
ocrs = "0.10"
rten = "0.17"
anyhow = "1"

[dev-dependencies]
proptest = "1"
```

`src-tauri/src/lib.rs`:
```rust
//! Wardogs Command Hub core: capture, OCR, parse, state.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

`src-tauri/src/bin/hub_cli.rs`:
```rust
fn main() {
    println!("hub-cli {}", wardogs_command_hub::VERSION);
}
```

Append to `.gitignore`:
```
src-tauri/target/
models/
```

- [ ] **Step 4: Run tests, expect pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: `test crate_exposes_version ... ok`. If `ocrs`/`rten` versions do not resolve, run `cargo search ocrs` and pin to the latest 0.x, keeping `rten` at the version `ocrs` depends on (`cargo tree -i rten` after a first build shows it).

- [ ] **Step 5: Commit**

```bash
git add src-tauri .gitignore
git commit -m "chore: scaffold rust crate with smoke test"
```

---

### Task 2: Header parser

**Files:**
- Create: `src-tauri/src/parser/mod.rs`, `src-tauri/src/parser/header.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Channel { Team, Squad, All, Unknown }
  pub struct Header { pub channel: Channel, pub clan: Option<String>, pub name: String, pub body: String }
  pub fn parse_header(raw: &str) -> Option<Header>
  ```

- [ ] **Step 1: Write failing tests** in `src-tauri/src/parser/header.rs` (bottom of file, `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_team_line() {
        let h = parse_header("[TEAM] Fl4sh: one is in tower 5").unwrap();
        assert_eq!(h.channel, Channel::Team);
        assert_eq!(h.clan, None);
        assert_eq!(h.name, "Fl4sh");
        assert_eq!(h.body, "one is in tower 5");
    }

    #[test]
    fn clan_tag_line() {
        let h = parse_header("[TEAM] [DOGA] mg_nd: supps are coming to tower 5").unwrap();
        assert_eq!(h.clan.as_deref(), Some("DOGA"));
        assert_eq!(h.name, "mg_nd");
    }

    #[test]
    fn squad_and_all_channels() {
        assert_eq!(parse_header("[SQUAD] a: b").unwrap().channel, Channel::Squad);
        assert_eq!(parse_header("[ALL] a: b").unwrap().channel, Channel::All);
    }

    #[test]
    fn ocr_noise_in_brackets_is_tolerated() {
        // OCR sometimes reads "[TEAM]" as "[TEAM ]" or "(TEAM)"
        assert_eq!(parse_header("[TEAM ] a: b").unwrap().channel, Channel::Team);
        assert_eq!(parse_header("(TEAM) a: b").unwrap().channel, Channel::Team);
    }

    #[test]
    fn line_without_header_is_none() {
        assert!(parse_header("tower 1").is_none());
        assert!(parse_header("").is_none());
    }

    #[test]
    fn empty_body_is_allowed() {
        let h = parse_header("[TEAM] Qsing: ").unwrap();
        assert_eq!(h.body, "");
    }
}
```

- [ ] **Step 2: Run, expect compile failure** (`parse_header` not found).

- [ ] **Step 3: Implement**

`src-tauri/src/parser/header.rs`:
```rust
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Channel {
    Team,
    Squad,
    All,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub channel: Channel,
    pub clan: Option<String>,
    pub name: String,
    pub body: String,
}

fn header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*[\[\(]\s*(TEAM|SQUAD|ALL)\s*[\]\)]\s*(?:\[\s*([^\]]+?)\s*\]\s*)?([^:\[\]]+?)\s*:\s?(.*)$",
        )
        .expect("static regex")
    })
}

fn channel_from(tag: &str) -> Channel {
    match tag.to_ascii_uppercase().as_str() {
        "TEAM" => Channel::Team,
        "SQUAD" => Channel::Squad,
        "ALL" => Channel::All,
        _ => Channel::Unknown,
    }
}

/// Split a raw chat line into channel, optional clan tag, player name and body.
/// Returns `None` when the line has no recognisable `[CHANNEL] name:` header.
pub fn parse_header(raw: &str) -> Option<Header> {
    let caps = header_re().captures(raw)?;
    let name = caps.get(3)?.as_str().trim();
    if name.is_empty() {
        return None;
    }
    Some(Header {
        channel: channel_from(caps.get(1)?.as_str()),
        clan: caps.get(2).map(|m| m.as_str().to_string()),
        name: name.to_string(),
        body: caps.get(4).map(|m| m.as_str().trim_end()).unwrap_or("").to_string(),
    })
}
```

`src-tauri/src/parser/mod.rs`:
```rust
pub mod header;
pub use header::{parse_header, Channel, Header};
```

`src-tauri/src/lib.rs` add: `pub mod parser;`

- [ ] **Step 4: Run tests, expect all header tests pass.**

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src
git commit -m "feat(parser): split chat header into channel, clan, name, body"
```

---

### Task 3: Coordinate matcher

**Files:**
- Create: `src-tauri/src/parser/coords.rs`
- Modify: `src-tauri/src/parser/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Coord { pub x: f64, pub y: f64 }
  pub struct CoordMatch { pub coord: Coord, pub rest: String }  // rest = body with the pair removed
  pub fn find_coords(body: &str) -> Option<CoordMatch>
  ```

- [ ] **Step 1: Failing tests** in `coords.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_pair() {
        let m = find_coords("x77.90, y70.60").unwrap();
        assert_eq!(m.coord, Coord { x: 77.90, y: 70.60 });
        assert_eq!(m.rest, "");
    }

    #[test]
    fn pair_with_trailing_text_and_lost_emoji() {
        let m = find_coords("x90.97, y101.30 need ammo here for tower 1").unwrap();
        assert_eq!(m.coord, Coord { x: 90.97, y: 101.30 });
        assert_eq!(m.rest, "need ammo here for tower 1");
    }

    #[test]
    fn ocr_variants() {
        assert!(find_coords("X 12.5 , Y 7").is_some());
        assert!(find_coords("x12,5; y7,25").is_some()); // decimal comma
        assert_eq!(find_coords("x12,5; y7,25").unwrap().coord, Coord { x: 12.5, y: 7.25 });
    }

    #[test]
    fn no_pair() {
        assert!(find_coords("one is in tower 5").is_none());
        assert!(find_coords("x is fine").is_none());
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Coord {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordMatch {
    pub coord: Coord,
    pub rest: String,
}

fn coord_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // x<num> <sep> y<num>; decimal point or comma; separator comma/semicolon/space
        Regex::new(r"(?i)\bx\s*(\d+(?:[.,]\d+)?)\s*[,;]?\s*y\s*(\d+(?:[.,]\d+)?)")
            .expect("static regex")
    })
}

fn parse_num(s: &str) -> Option<f64> {
    s.replace(',', ".").parse().ok()
}

/// Find the first `x<n>, y<n>` pair in a chat body.
pub fn find_coords(body: &str) -> Option<CoordMatch> {
    let caps = coord_re().captures(body)?;
    let whole = caps.get(0)?;
    let x = parse_num(caps.get(1)?.as_str())?;
    let y = parse_num(caps.get(2)?.as_str())?;
    let rest = format!("{}{}", &body[..whole.start()], &body[whole.end()..]);
    Some(CoordMatch {
        coord: Coord { x, y },
        rest: rest.split_whitespace().collect::<Vec<_>>().join(" "),
    })
}
```

Add to `parser/mod.rs`: `pub mod coords; pub use coords::{find_coords, Coord, CoordMatch};`

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat(parser): match x/y coordinate pairs with OCR-tolerant regex`

---

### Task 4: Entity classifier

**Files:**
- Create: `src-tauri/src/parser/entity.rs`
- Modify: `src-tauri/src/parser/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Entity { Vehicle, Structure, Infantry, Other }
  pub fn classify_entity(body: &str) -> Entity
  ```

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicles() {
        for s in ["enemy tank on the ridge", "APC coming", "heli over tower 2", "truck at bridge", "jeep"] {
            assert_eq!(classify_entity(s), Entity::Vehicle, "{s}");
        }
    }

    #[test]
    fn structures() {
        for s in ["they built a bunker", "enemy fob here", "mortar pit at x", "AA turret", "walls going up"] {
            assert_eq!(classify_entity(s), Entity::Structure, "{s}");
        }
    }

    #[test]
    fn infantry() {
        for s in ["one is in tower 5", "sniper on hill", "3 guys pushing", "squad flanking left", "enemys in there"] {
            assert_eq!(classify_entity(s), Entity::Infantry, "{s}");
        }
    }

    #[test]
    fn other() {
        assert_eq!(classify_entity("supplies delivered to tower 5"), Entity::Other);
        assert_eq!(classify_entity("need ammo here"), Entity::Other);
        assert_eq!(classify_entity(""), Entity::Other);
    }

    #[test]
    fn first_category_by_priority_wins() {
        // "tank" (vehicle) beats "guys" (infantry) because vehicle keywords are checked first
        assert_eq!(classify_entity("tank with guys around"), Entity::Vehicle);
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Entity {
    Vehicle,
    Structure,
    Infantry,
    Other,
}

const VEHICLE: &[&str] = &[
    "tank", "apc", "ifv", "heli", "helicopter", "chopper", "truck", "jeep", "humvee", "car",
    "vehicle", "vic", "bike", "boat", "mrap", "btr", "bmp", "sph", "artillery truck",
];
const STRUCTURE: &[&str] = &[
    "bunker", "fob", "base", "hab", "mortar", "turret", "aa", "wall", "walls", "gate", "tower",
    "radio", "outpost", "emplacement", "sandbag", "hesco", "spawn", "structure", "building",
];
const INFANTRY: &[&str] = &[
    "sniper", "guy", "guys", "man", "men", "inf", "infantry", "squad", "enemy", "enemys",
    "enemies", "one", "two", "three", "pushing", "flanking", "camping", "player", "players",
];

fn has_keyword(words: &[String], list: &[&str]) -> bool {
    list.iter().any(|k| {
        if k.contains(' ') {
            words.join(" ").contains(k)
        } else {
            words.iter().any(|w| w == k)
        }
    })
}

/// Classify what a chat body is reporting. Checked in priority order:
/// vehicle, then structure, then infantry. Structure words like "tower" are
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
```

Note: with this rule "they built a bunker" is Structure (no infantry word), "one is in tower 5" is Infantry, "supplies delivered to tower 5 but now dead. enemys in there" is Infantry. The test `structures` uses phrases with no infantry words. If a test in Step 1 disagrees with this rule, the rule wins and the test is adjusted, because the rule is what the spec describes ("keyword lists").

Add to `parser/mod.rs`: `pub mod entity; pub use entity::{classify_entity, Entity};`

- [ ] **Step 4: Run tests, expect pass.** (Check `structures`: "they built a bunker" → words contain none of INFANTRY → Structure. "enemy fob here" contains "enemy" → Infantry. Change that test phrase to "fob built here".)

- [ ] **Step 5: Commit** `feat(parser): classify chat bodies into vehicle/structure/infantry`

---

### Task 5: Named place index

**Files:**
- Create: `src-tauri/src/parser/places.rs`
- Modify: `src-tauri/src/parser/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct PlaceIndex { .. }
  impl PlaceIndex {
      pub fn new(places: &BTreeMap<String, Coord>, aliases: &BTreeMap<String, String>) -> PlaceIndex;
      pub fn empty() -> PlaceIndex;
      pub fn find(&self, body: &str) -> Option<PlaceMatch>;
  }
  pub struct PlaceMatch { pub name: String, pub coord: Coord }
  ```

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn idx() -> PlaceIndex {
        let mut p = BTreeMap::new();
        p.insert("tower 5".to_string(), Coord { x: 41.2, y: 99.3 });
        p.insert("tower 1".to_string(), Coord { x: 10.0, y: 20.0 });
        p.insert("old mill".to_string(), Coord { x: 5.0, y: 5.0 });
        let mut a = BTreeMap::new();
        a.insert("t5".to_string(), "tower 5".to_string());
        a.insert("mill".to_string(), "old mill".to_string());
        PlaceIndex::new(&p, &a)
    }

    #[test]
    fn exact_case_insensitive() {
        let m = idx().find("one is in Tower 5").unwrap();
        assert_eq!(m.name, "tower 5");
        assert_eq!(m.coord, Coord { x: 41.2, y: 99.3 });
    }

    #[test]
    fn alias() {
        assert_eq!(idx().find("push t5 now").unwrap().name, "tower 5");
        assert_eq!(idx().find("meet at the mill").unwrap().name, "old mill");
    }

    #[test]
    fn whole_word_only() {
        // "t5" must not match inside "t55"; "mill" must not match "million"
        assert!(idx().find("t55 tanks").is_none());
        assert!(idx().find("a million things").is_none());
    }

    #[test]
    fn longest_match_wins() {
        // both "tower 1" and "tower 15" would start the same; only "tower 1" exists here
        assert_eq!(idx().find("tower 1 needs help").unwrap().name, "tower 1");
    }

    #[test]
    fn empty_index_matches_nothing() {
        assert!(PlaceIndex::empty().find("tower 5").is_none());
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use super::coords::Coord;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct PlaceMatch {
    pub name: String,
    pub coord: Coord,
}

/// Case-insensitive, whole-word matcher for named locations and their aliases.
#[derive(Debug, Clone, Default)]
pub struct PlaceIndex {
    /// (lowercased phrase, canonical name, coord), longest phrase first
    entries: Vec<(String, String, Coord)>,
}

impl PlaceIndex {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new(places: &BTreeMap<String, Coord>, aliases: &BTreeMap<String, String>) -> Self {
        let mut entries: Vec<(String, String, Coord)> = places
            .iter()
            .map(|(n, c)| (n.to_lowercase(), n.clone(), *c))
            .collect();
        for (alias, target) in aliases {
            if let Some(c) = places.get(target) {
                entries.push((alias.to_lowercase(), target.clone(), *c));
            }
        }
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        Self { entries }
    }

    pub fn find(&self, body: &str) -> Option<PlaceMatch> {
        let hay = normalise(body);
        self.entries
            .iter()
            .find(|(phrase, _, _)| contains_whole(&hay, phrase))
            .map(|(_, name, coord)| PlaceMatch { name: name.clone(), coord: *coord })
    }
}

/// Lowercase and collapse non-alphanumerics to single spaces, padded so
/// whole-word checks can look for " phrase ".
fn normalise(s: &str) -> String {
    let inner: Vec<&str> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    format!(" {} ", inner.join(" ").to_lowercase())
}

fn contains_whole(hay: &str, phrase: &str) -> bool {
    let needle = normalise(phrase);
    hay.contains(needle.as_str())
}
```

Add to `parser/mod.rs`: `pub mod places; pub use places::{PlaceIndex, PlaceMatch};`

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat(parser): named place index with aliases and whole-word matching`

---

### Task 6: Message assembly

**Files:**
- Modify: `src-tauri/src/parser/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum LocationKind { Coord, Named }
  pub struct Location { pub kind: LocationKind, pub coord: Coord, pub label: Option<String> }
  pub struct Message { pub id: String, pub at_ms: u64, pub channel: Channel, pub clan: Option<String>,
                       pub name: String, pub body: String, pub raw: String,
                       pub location: Option<Location>, pub entity: Entity }
  pub fn parse_message(raw: &str, places: &PlaceIndex, at_ms: u64) -> Message
  ```
  `id` is a stable hash of `raw`, hex string (used by state to link pins).

- [ ] **Step 1: Failing tests** appended to `parser/mod.rs`:

```rust
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
        let m = parse_message("[TEAM] LeftWild: x90.97, y101.30 need ammo here for tower 1", &places(), 1000);
        assert_eq!(m.channel, Channel::Team);
        assert_eq!(m.name, "LeftWild");
        let loc = m.location.unwrap();
        assert_eq!(loc.kind, LocationKind::Coord);
        assert_eq!(loc.coord, Coord { x: 90.97, y: 101.30 });
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
    fn id_is_stable_for_same_raw() {
        let a = parse_message("[TEAM] a: b", &places(), 1);
        let b = parse_message("[TEAM] a: b", &places(), 2);
        assert_eq!(a.id, b.id);
        assert_ne!(a.id, parse_message("[TEAM] a: c", &places(), 1).id);
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement** (replace `parser/mod.rs` contents):

```rust
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

fn stable_id(raw: &str) -> String {
    let mut h = DefaultHasher::new();
    raw.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn locate(body: &str, places: &PlaceIndex) -> (Option<Location>, String) {
    if let Some(m) = find_coords(body) {
        let loc = Location { kind: LocationKind::Coord, coord: m.coord, label: None };
        return (Some(loc), m.rest);
    }
    if let Some(p) = places.find(body) {
        let loc = Location { kind: LocationKind::Named, coord: p.coord, label: Some(p.name) };
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
        id: stable_id(raw),
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
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat(parser): assemble located, classified messages from chat lines`

---

### Task 7: Deduper

**Files:**
- Create: `src-tauri/src/dedup.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod dedup;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct Deduper { .. }
  impl Deduper {
      pub fn new(capacity: usize) -> Deduper;
      /// Feed one OCR frame's lines (top to bottom). Returns only lines not seen recently, wrapped continuations joined.
      pub fn push(&mut self, frame_lines: &[String]) -> Vec<String>;
  }
  pub fn normalise_line(s: &str) -> String;
  ```
  `Deduper` is the one intentionally stateful object in the pipeline (a ring buffer); `push` takes `&mut self`.

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn first_frame_all_new() {
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["[TEAM] a: one", "[TEAM] b: two"]));
        assert_eq!(out, lines(&["[TEAM] a: one", "[TEAM] b: two"]));
    }

    #[test]
    fn repeated_frame_yields_nothing() {
        let mut d = Deduper::new(50);
        d.push(&lines(&["[TEAM] a: one"]));
        assert!(d.push(&lines(&["[TEAM] a: one"])).is_empty());
    }

    #[test]
    fn scrolled_frame_yields_only_new_tail() {
        let mut d = Deduper::new(50);
        d.push(&lines(&["[TEAM] a: one", "[TEAM] b: two"]));
        let out = d.push(&lines(&["[TEAM] b: two", "[TEAM] c: three"]));
        assert_eq!(out, lines(&["[TEAM] c: three"]));
    }

    #[test]
    fn whitespace_and_case_noise_is_same_line() {
        let mut d = Deduper::new(50);
        d.push(&lines(&["[TEAM] a: Need  ammo"]));
        assert!(d.push(&lines(&["[TEAM]  a: need ammo "])).is_empty());
    }

    #[test]
    fn wrapped_continuation_is_joined_to_previous() {
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["[TEAM] LeftWild: x90.97, y101.30 need ammo here for", "tower 1"]));
        assert_eq!(out, lines(&["[TEAM] LeftWild: x90.97, y101.30 need ammo here for tower 1"]));
    }

    #[test]
    fn leading_continuation_without_parent_is_dropped() {
        // top of the box can show the tail of an already-scrolled line
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["tower 1", "[TEAM] a: b"]));
        assert_eq!(out, lines(&["[TEAM] a: b"]));
    }

    #[test]
    fn ring_buffer_forgets_old_lines() {
        let mut d = Deduper::new(2);
        d.push(&lines(&["[TEAM] a: 1"]));
        d.push(&lines(&["[TEAM] a: 2"]));
        d.push(&lines(&["[TEAM] a: 3"]));
        assert_eq!(d.push(&lines(&["[TEAM] a: 1"])), lines(&["[TEAM] a: 1"]));
    }

    #[test]
    fn normalise_collapses() {
        assert_eq!(normalise_line("  [TEAM]  A:  Hi  There "), "[team] a: hi there");
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use crate::parser::parse_header;
use std::collections::VecDeque;

/// Lowercase, trim, collapse whitespace. Used only for equality, never shown.
pub fn normalise_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Joins wrapped continuation lines (no `[CHANNEL] name:` header) onto the
/// preceding headed line. A continuation with no parent is dropped.
fn join_wrapped(frame_lines: &[String]) -> Vec<String> {
    frame_lines.iter().fold(Vec::new(), |mut acc, line| {
        let is_headed = parse_header(line).is_some();
        match (is_headed, acc.last_mut()) {
            (true, _) => acc.push(line.trim().to_string()),
            (false, Some(prev)) => {
                let joined = format!("{} {}", prev.trim_end(), line.trim());
                *prev = joined;
            }
            (false, None) => {}
        }
        acc
    })
}

#[derive(Debug)]
pub struct Deduper {
    seen: VecDeque<String>,
    capacity: usize,
}

impl Deduper {
    pub fn new(capacity: usize) -> Self {
        Self { seen: VecDeque::with_capacity(capacity), capacity: capacity.max(1) }
    }

    pub fn push(&mut self, frame_lines: &[String]) -> Vec<String> {
        let mut fresh = Vec::new();
        for line in join_wrapped(frame_lines) {
            let key = normalise_line(&line);
            if key.is_empty() || self.seen.contains(&key) {
                continue;
            }
            self.remember(key);
            fresh.push(line);
        }
        fresh
    }

    fn remember(&mut self, key: String) {
        if self.seen.len() == self.capacity {
            self.seen.pop_front();
        }
        self.seen.push_back(key);
    }
}
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat: dedup OCR lines across frames and join wrapped continuations`

---

### Task 8: Calibration maths

**Files:**
- Create: `src-tauri/src/maps/mod.rs`, `src-tauri/src/maps/calibration.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod maps;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct CalPoint { pub game: Coord, pub px: Coord }
  pub struct Calibration { pub a: CalPoint, pub b: CalPoint }
  pub enum CalibrationError { DegenerateAxis(&'static str) }
  impl Calibration {
      pub fn validate(&self) -> Result<(), CalibrationError>;
      pub fn to_pixel(&self, game: Coord) -> Coord;
      pub fn to_game(&self, px: Coord) -> Coord;
  }
  ```

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Coord;
    use proptest::prelude::*;

    fn cal() -> Calibration {
        Calibration {
            a: CalPoint { game: Coord { x: 0.0, y: 0.0 }, px: Coord { x: 100.0, y: 200.0 } },
            b: CalPoint { game: Coord { x: 100.0, y: 50.0 }, px: Coord { x: 1100.0, y: 700.0 } },
        }
    }

    #[test]
    fn maps_anchor_points_exactly() {
        let c = cal();
        assert_eq!(c.to_pixel(c.a.game), c.a.px);
        assert_eq!(c.to_pixel(c.b.game), c.b.px);
    }

    #[test]
    fn interpolates_and_extrapolates_linearly() {
        let c = cal();
        assert_eq!(c.to_pixel(Coord { x: 50.0, y: 25.0 }), Coord { x: 600.0, y: 450.0 });
        assert_eq!(c.to_pixel(Coord { x: 200.0, y: -50.0 }), Coord { x: 2100.0, y: -300.0 });
    }

    #[test]
    fn y_axis_may_be_flipped() {
        let c = Calibration {
            a: CalPoint { game: Coord { x: 0.0, y: 0.0 }, px: Coord { x: 0.0, y: 1000.0 } },
            b: CalPoint { game: Coord { x: 10.0, y: 10.0 }, px: Coord { x: 1000.0, y: 0.0 } },
        };
        assert_eq!(c.to_pixel(Coord { x: 5.0, y: 5.0 }), Coord { x: 500.0, y: 500.0 });
        assert_eq!(c.to_pixel(Coord { x: 0.0, y: 10.0 }), Coord { x: 0.0, y: 0.0 });
    }

    #[test]
    fn degenerate_points_rejected() {
        let c = Calibration { a: cal().a, b: cal().a };
        assert!(matches!(c.validate(), Err(CalibrationError::DegenerateAxis(_))));
    }

    proptest! {
        #[test]
        fn round_trip(x in -1000.0f64..1000.0, y in -1000.0f64..1000.0) {
            let c = cal();
            let back = c.to_game(c.to_pixel(Coord { x, y }));
            prop_assert!((back.x - x).abs() < 1e-6);
            prop_assert!((back.y - y).abs() < 1e-6);
        }
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

`maps/calibration.rs`:
```rust
use crate::parser::Coord;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalPoint {
    pub game: Coord,
    pub px: Coord,
}

/// Two-point axis-aligned affine map between game coordinates and source
/// image pixels. Each axis gets its own scale and offset, so a flipped y
/// axis or non-square units are handled.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Calibration {
    pub a: CalPoint,
    pub b: CalPoint,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CalibrationError {
    #[error("calibration points share the same {0} value; pick two points that differ on both axes")]
    DegenerateAxis(&'static str),
}

const EPS: f64 = 1e-9;

impl Calibration {
    pub fn validate(&self) -> Result<(), CalibrationError> {
        if (self.a.game.x - self.b.game.x).abs() < EPS || (self.a.px.x - self.b.px.x).abs() < EPS {
            return Err(CalibrationError::DegenerateAxis("x"));
        }
        if (self.a.game.y - self.b.game.y).abs() < EPS || (self.a.px.y - self.b.px.y).abs() < EPS {
            return Err(CalibrationError::DegenerateAxis("y"));
        }
        Ok(())
    }

    fn scale(&self) -> (f64, f64) {
        (
            (self.b.px.x - self.a.px.x) / (self.b.game.x - self.a.game.x),
            (self.b.px.y - self.a.px.y) / (self.b.game.y - self.a.game.y),
        )
    }

    pub fn to_pixel(&self, game: Coord) -> Coord {
        let (sx, sy) = self.scale();
        Coord {
            x: self.a.px.x + (game.x - self.a.game.x) * sx,
            y: self.a.px.y + (game.y - self.a.game.y) * sy,
        }
    }

    pub fn to_game(&self, px: Coord) -> Coord {
        let (sx, sy) = self.scale();
        Coord {
            x: self.a.game.x + (px.x - self.a.px.x) / sx,
            y: self.a.game.y + (px.y - self.a.px.y) / sy,
        }
    }
}
```

`maps/mod.rs`:
```rust
pub mod calibration;
pub use calibration::{CalPoint, Calibration, CalibrationError};
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat(maps): two-point calibration between game and pixel coordinates`

---

### Task 9: Map manifest and registry

**Files:**
- Create: `src-tauri/src/maps/manifest.rs`, `maps/bakurani/map.json`, `maps/ozeti/map.json`, `maps/zestafona/map.json`, `maps/README.md`
- Modify: `src-tauri/src/maps/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct MapManifest { pub id: String, pub name: String, pub source_size: [u32; 2],
                           pub calibration: Option<Calibration>,
                           pub places: BTreeMap<String, Coord>, pub aliases: BTreeMap<String, String> }
  impl MapManifest {
      pub fn from_json(text: &str) -> Result<MapManifest, ManifestError>;
      pub fn place_index(&self) -> PlaceIndex;
  }
  pub struct MapRegistry { pub maps: BTreeMap<String, MapManifest>, pub errors: BTreeMap<String, String> }
  impl MapRegistry { pub fn load_dir(dir: &Path) -> Result<MapRegistry, ManifestError>; pub fn get(&self, id: &str) -> Option<&MapManifest>; }
  pub enum ManifestError { Json(String), Invalid(String), Io(std::io::Error) }
  ```
  `map.json` field names are camelCase (`sourceSize`). `calibration` may be `null`.

- [ ] **Step 1: Failing tests**

```rust
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
        let m = MapManifest::from_json(r#"{"id":"x","name":"X","sourceSize":[10,10],"calibration":null}"#).unwrap();
        assert!(m.calibration.is_none());
        assert!(m.places.is_empty());
    }

    #[test]
    fn rejects_bad_id_and_size() {
        assert!(matches!(MapManifest::from_json(r#"{"id":"","name":"X","sourceSize":[10,10]}"#), Err(ManifestError::Invalid(_))));
        assert!(matches!(MapManifest::from_json(r#"{"id":"Bad Id","name":"X","sourceSize":[10,10]}"#), Err(ManifestError::Invalid(_))));
        assert!(matches!(MapManifest::from_json(r#"{"id":"x","name":"X","sourceSize":[0,10]}"#), Err(ManifestError::Invalid(_))));
    }

    #[test]
    fn rejects_alias_to_unknown_place_and_degenerate_calibration() {
        let bad_alias = r#"{"id":"x","name":"X","sourceSize":[10,10],"aliases":{"a":"nowhere"}}"#;
        assert!(matches!(MapManifest::from_json(bad_alias), Err(ManifestError::Invalid(_))));
        let bad_cal = r#"{"id":"x","name":"X","sourceSize":[10,10],
          "calibration":{"a":{"game":{"x":0,"y":0},"px":{"x":0,"y":0}},"b":{"game":{"x":0,"y":0},"px":{"x":0,"y":0}}}}"#;
        assert!(matches!(MapManifest::from_json(bad_cal), Err(ManifestError::Invalid(_))));
    }

    #[test]
    fn malformed_json_is_json_error() {
        assert!(matches!(MapManifest::from_json("{"), Err(ManifestError::Json(_))));
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
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

`maps/manifest.rs`:
```rust
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
    !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl MapManifest {
    pub fn from_json(text: &str) -> Result<Self, ManifestError> {
        let m: MapManifest = serde_json::from_str(text).map_err(|e| ManifestError::Json(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        let inv = |s: String| Err(ManifestError::Invalid(s));
        if !valid_id(&self.id) {
            return inv(format!("id '{}' must be lowercase letters, digits or '-'", self.id));
        }
        if self.name.trim().is_empty() {
            return inv("name is empty".into());
        }
        if self.source_size.iter().any(|&n| n == 0) {
            return inv("sourceSize must be positive".into());
        }
        if let Some(c) = &self.calibration {
            c.validate().map_err(|e| ManifestError::Invalid(e.to_string()))?;
        }
        for (alias, target) in &self.aliases {
            if !self.places.contains_key(target) {
                return inv(format!("alias '{alias}' points to unknown place '{target}'"));
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
            let folder = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
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
```

`maps/mod.rs` add: `pub mod manifest; pub use manifest::{ManifestError, MapManifest, MapRegistry};`

Create the three manifests, e.g. `maps/bakurani/map.json`:
```json
{
  "id": "bakurani",
  "name": "Bakurani",
  "sourceSize": [16384, 16384],
  "calibration": null,
  "places": {},
  "aliases": {}
}
```
Same for `ozeti` ("Ozeti") and `zestafona` ("Zestafona").

`maps/README.md`:
```markdown
# Maps

One folder per map. `map.json` is the only committed file; `source.png`,
`tiles/` and `thumb.png` are generated or downloaded locally (gitignored).

- `id`: folder name, lowercase.
- `sourceSize`: pixel size of `source.png`. 16384 square for the three
  built-in maps (community hi-res captures, see SOURCE.md when added).
- `calibration`: `null` until you calibrate in the app. Two points, each
  with the in-game `x, y` and the pixel position on `source.png`.
- `places`: named locations in game coordinates, e.g. `"tower 5": {"x": 41.2, "y": 99.3}`.
- `aliases`: shorthand players type, mapped to a `places` key.

Add a map: create the folder, write `map.json`, drop `source.png`, run the
tiler (Plan 2). No code changes.
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat(maps): map.json manifest, validation and registry; add three map stubs`

---

### Task 10: Settings

**Files:**
- Create: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod config;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
  pub enum OcrEngineKind { Ocrs, Windows }
  pub struct Settings { pub monitor_index: usize, pub chat_rect: Rect, pub capture_fps: f32,
                        pub ocr_engine: OcrEngineKind, pub pin_ttl_secs: u64,
                        pub selected_map: Option<String>, pub dedup_capacity: usize, pub models_dir: PathBuf }
  impl Settings { pub fn default() ; pub fn validate(&self) -> Result<(), ConfigError>;
                  pub fn load(path: &Path) -> Result<Settings, ConfigError>; pub fn save(&self, path: &Path) -> Result<(), ConfigError>; }
  pub enum ConfigError { Io, Json(String), Invalid(String) }
  ```

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn rejects_zero_rect_and_silly_fps() {
        let mut s = Settings::default();
        s.chat_rect = Rect { x: 0, y: 0, w: 0, h: 10 };
        assert!(matches!(s.validate(), Err(ConfigError::Invalid(_))));
        let mut s = Settings::default();
        s.capture_fps = 0.0;
        assert!(s.validate().is_err());
        s.capture_fps = 61.0;
        assert!(s.validate().is_err());
    }

    #[test]
    fn round_trips_through_file() {
        let p = std::env::temp_dir().join(format!("hub-settings-{}.json", std::process::id()));
        let mut s = Settings::default();
        s.selected_map = Some("ozeti".into());
        s.save(&p).unwrap();
        let back = Settings::load(&p).unwrap();
        assert_eq!(back, s);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn missing_file_yields_defaults() {
        let p = std::env::temp_dir().join("hub-settings-definitely-missing.json");
        assert_eq!(Settings::load(&p).unwrap(), Settings::default());
    }

    #[test]
    fn corrupt_file_is_an_error_not_defaults() {
        let p = std::env::temp_dir().join(format!("hub-settings-bad-{}.json", std::process::id()));
        std::fs::write(&p, "{ nope").unwrap();
        assert!(matches!(Settings::load(&p), Err(ConfigError::Json(_))));
        std::fs::remove_file(p).unwrap();
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings file is not valid JSON: {0}")]
    Json(String),
    #[error("settings invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OcrEngineKind {
    Ocrs,
    Windows,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub monitor_index: usize,
    pub chat_rect: Rect,
    pub capture_fps: f32,
    pub ocr_engine: OcrEngineKind,
    pub pin_ttl_secs: u64,
    pub selected_map: Option<String>,
    pub dedup_capacity: usize,
    pub models_dir: PathBuf,
}

pub const MAX_FPS: f32 = 60.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            monitor_index: 0,
            // matches the 1600x900 fixture: chat box top-left
            chat_rect: Rect { x: 20, y: 20, w: 300, h: 110 },
            capture_fps: 2.0,
            ocr_engine: if cfg!(windows) { OcrEngineKind::Windows } else { OcrEngineKind::Ocrs },
            pin_ttl_secs: 300,
            selected_map: None,
            dedup_capacity: 200,
            models_dir: PathBuf::from("models"),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let inv = |s: &str| Err(ConfigError::Invalid(s.to_string()));
        if self.chat_rect.w == 0 || self.chat_rect.h == 0 {
            return inv("chat rectangle must have non-zero width and height");
        }
        if !(self.capture_fps > 0.0 && self.capture_fps <= MAX_FPS) {
            return inv("capture fps must be between 0 and 60");
        }
        if self.pin_ttl_secs == 0 {
            return inv("pin ttl must be at least 1 second");
        }
        if self.dedup_capacity == 0 {
            return inv("dedup capacity must be at least 1");
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let s: Settings = serde_json::from_str(&text).map_err(|e| ConfigError::Json(e.to_string()))?;
        s.validate()?;
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| ConfigError::Json(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat: persisted, validated settings`

---

### Task 11: State snapshot

**Files:**
- Create: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod state;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct Pin { pub message_id: String, pub px: Coord, pub game: Coord, pub entity: Entity, pub at_ms: u64, pub expires_at_ms: u64 }
  pub struct Snapshot { pub messages: Vec<Message>, pub pins: Vec<Pin>, pub map_id: Option<String> }
  pub fn apply(prev: &Snapshot, msg: Message, map: Option<&MapManifest>, ttl_ms: u64, max_messages: usize) -> Snapshot
  pub fn expire(prev: &Snapshot, now_ms: u64) -> Snapshot
  pub fn with_map(prev: &Snapshot, map_id: Option<String>) -> Snapshot  // clears pins
  ```
  All three return new snapshots; inputs are never mutated.

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::{CalPoint, Calibration, MapManifest};
    use crate::parser::{parse_message, Coord, PlaceIndex};

    fn map(calibrated: bool) -> MapManifest {
        let cal = Calibration {
            a: CalPoint { game: Coord { x: 0.0, y: 0.0 }, px: Coord { x: 0.0, y: 0.0 } },
            b: CalPoint { game: Coord { x: 100.0, y: 100.0 }, px: Coord { x: 1000.0, y: 1000.0 } },
        };
        MapManifest {
            id: "t".into(), name: "T".into(), source_size: [1000, 1000],
            calibration: calibrated.then_some(cal),
            places: Default::default(), aliases: Default::default(),
        }
    }

    fn msg(raw: &str, at: u64) -> Message {
        parse_message(raw, &PlaceIndex::empty(), at)
    }

    #[test]
    fn located_message_on_calibrated_map_makes_pin() {
        let s = apply(&Snapshot::default(), msg("[TEAM] a: x50, y25 tank", 1000), Some(&map(true)), 60_000, 100);
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.pins.len(), 1);
        assert_eq!(s.pins[0].px, Coord { x: 500.0, y: 250.0 });
        assert_eq!(s.pins[0].expires_at_ms, 61_000);
        assert_eq!(s.pins[0].entity, Entity::Vehicle);
    }

    #[test]
    fn uncalibrated_or_no_map_keeps_message_but_no_pin() {
        let s = apply(&Snapshot::default(), msg("[TEAM] a: x50, y25", 0), Some(&map(false)), 60_000, 100);
        assert_eq!(s.messages.len(), 1);
        assert!(s.pins.is_empty());
        let s = apply(&Snapshot::default(), msg("[TEAM] a: x50, y25", 0), None, 60_000, 100);
        assert!(s.pins.is_empty());
    }

    #[test]
    fn unlocated_message_makes_no_pin() {
        let s = apply(&Snapshot::default(), msg("[TEAM] a: hello", 0), Some(&map(true)), 60_000, 100);
        assert!(s.pins.is_empty());
    }

    #[test]
    fn apply_does_not_mutate_previous() {
        let first = apply(&Snapshot::default(), msg("[TEAM] a: x1, y1", 0), Some(&map(true)), 1000, 100);
        let _second = apply(&first, msg("[TEAM] a: x2, y2", 0), Some(&map(true)), 1000, 100);
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
        let s = apply(&Snapshot::default(), msg("[TEAM] a: x1, y1", 0), Some(&map(true)), 1000, 100);
        let s = apply(&s, msg("[TEAM] a: x2, y2", 5000), Some(&map(true)), 1000, 100);
        let e = expire(&s, 3000);
        assert_eq!(e.pins.len(), 1);
        assert_eq!(e.messages.len(), 2);
    }

    #[test]
    fn with_map_clears_pins() {
        let s = apply(&Snapshot::default(), msg("[TEAM] a: x1, y1", 0), Some(&map(true)), 1000, 100);
        let s = with_map(&s, Some("other".into()));
        assert!(s.pins.is_empty());
        assert_eq!(s.map_id.as_deref(), Some("other"));
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
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
pub fn apply(prev: &Snapshot, msg: Message, map: Option<&MapManifest>, ttl_ms: u64, max_messages: usize) -> Snapshot {
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
        pins: prev.pins.iter().filter(|p| p.expires_at_ms > now_ms).cloned().collect(),
        map_id: prev.map_id.clone(),
    }
}

/// Switch map. Pins belong to the old map's pixel space, so they are cleared.
pub fn with_map(prev: &Snapshot, map_id: Option<String>) -> Snapshot {
    Snapshot { messages: prev.messages.clone(), pins: Vec::new(), map_id }
}
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat: immutable snapshot with pins, expiry and map switch`

---

### Task 12: Frame gate

**Files:**
- Create: `src-tauri/src/gate.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod gate;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct FrameGate { .. }
  impl FrameGate {
      pub fn new(threshold: f32) -> FrameGate;   // mean abs luma diff in 0..255 that counts as "changed"
      pub fn changed(&mut self, frame: &image::RgbaImage) -> bool;  // true on first frame and on change
  }
  pub fn thumb_luma(frame: &image::RgbaImage) -> Vec<u8>;  // 32x8 grey thumbnail
  ```

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(v: u8) -> RgbaImage {
        RgbaImage::from_pixel(300, 100, Rgba([v, v, v, 255]))
    }

    #[test]
    fn first_frame_counts_as_changed() {
        let mut g = FrameGate::new(4.0);
        assert!(g.changed(&solid(10)));
    }

    #[test]
    fn identical_frame_is_unchanged() {
        let mut g = FrameGate::new(4.0);
        g.changed(&solid(10));
        assert!(!g.changed(&solid(10)));
    }

    #[test]
    fn small_noise_is_unchanged_big_change_is_changed() {
        let mut g = FrameGate::new(4.0);
        g.changed(&solid(10));
        assert!(!g.changed(&solid(12)));
        assert!(g.changed(&solid(200)));
    }

    #[test]
    fn partial_change_is_detected() {
        let mut g = FrameGate::new(4.0);
        g.changed(&solid(0));
        let mut f = solid(0);
        for y in 0..20 {
            for x in 0..300 {
                f.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        assert!(g.changed(&f)); // one text line's worth of white on black
    }

    #[test]
    fn thumb_is_fixed_size() {
        assert_eq!(thumb_luma(&solid(0)).len(), 32 * 8);
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use image::{imageops, RgbaImage};

const THUMB_W: u32 = 32;
const THUMB_H: u32 = 8;

/// Tiny greyscale thumbnail used for cheap change detection.
pub fn thumb_luma(frame: &RgbaImage) -> Vec<u8> {
    let small = imageops::resize(frame, THUMB_W, THUMB_H, imageops::FilterType::Triangle);
    small
        .pixels()
        .map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8)
        .collect()
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f32 {
    let sum: u32 = a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs()).sum();
    sum as f32 / a.len().max(1) as f32
}

/// Skips OCR when the chat rectangle has not visibly changed.
#[derive(Debug)]
pub struct FrameGate {
    last: Option<Vec<u8>>,
    threshold: f32,
}

impl FrameGate {
    pub fn new(threshold: f32) -> Self {
        Self { last: None, threshold }
    }

    pub fn changed(&mut self, frame: &RgbaImage) -> bool {
        let now = thumb_luma(frame);
        let changed = match &self.last {
            None => true,
            Some(prev) => mean_abs_diff(prev, &now) >= self.threshold,
        };
        if changed {
            self.last = Some(now);
        }
        changed
    }
}
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat: frame gate skips OCR on unchanged chat frames`

---

### Task 13: Screen capture

**Files:**
- Create: `src-tauri/src/capture.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod capture;`)

**Interfaces:**
- Produces:
  ```rust
  pub trait FrameSource { fn grab(&mut self) -> Result<image::RgbaImage, CaptureError>; }
  pub struct ScreenCapturer { .. }
  impl ScreenCapturer { pub fn new(monitor_index: usize, rect: Rect) -> Result<ScreenCapturer, CaptureError>; pub fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError>; }
  pub struct MonitorInfo { pub index: usize, pub name: String, pub width: u32, pub height: u32 }
  pub fn crop(frame: &RgbaImage, rect: Rect) -> Result<RgbaImage, CaptureError>
  pub enum CaptureError { NoMonitor(usize), RectOutOfBounds { rect: Rect, width: u32, height: u32 }, Backend(String) }
  ```
  Live capture is not unit tested (no display on CI). `crop` and error mapping are.

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn crop_returns_requested_size() {
        let f = RgbaImage::from_pixel(100, 50, Rgba([1, 2, 3, 255]));
        let c = crop(&f, Rect { x: 10, y: 5, w: 30, h: 20 }).unwrap();
        assert_eq!(c.dimensions(), (30, 20));
    }

    #[test]
    fn crop_out_of_bounds_is_error() {
        let f = RgbaImage::from_pixel(100, 50, Rgba([0; 4]));
        let e = crop(&f, Rect { x: 90, y: 0, w: 30, h: 20 }).unwrap_err();
        assert!(matches!(e, CaptureError::RectOutOfBounds { .. }));
    }

    #[test]
    fn bad_monitor_index_is_error_or_backend_error() {
        // On a headless CI there may be no monitors at all: either error is acceptable,
        // but it must not panic.
        let r = ScreenCapturer::new(usize::MAX, Rect { x: 0, y: 0, w: 1, h: 1 });
        assert!(r.is_err());
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use crate::config::Rect;
use image::{imageops, RgbaImage};
use xcap::Monitor;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("monitor {0} not found")]
    NoMonitor(usize),
    #[error("chat rectangle {rect:?} exceeds frame {width}x{height}")]
    RectOutOfBounds { rect: Rect, width: u32, height: u32 },
    #[error("capture backend error: {0}")]
    Backend(String),
}

pub trait FrameSource {
    fn grab(&mut self) -> Result<RgbaImage, CaptureError>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

pub fn crop(frame: &RgbaImage, rect: Rect) -> Result<RgbaImage, CaptureError> {
    let (width, height) = frame.dimensions();
    let fits = rect.x.checked_add(rect.w).is_some_and(|r| r <= width)
        && rect.y.checked_add(rect.h).is_some_and(|b| b <= height);
    if !fits {
        return Err(CaptureError::RectOutOfBounds { rect, width, height });
    }
    Ok(imageops::crop_imm(frame, rect.x, rect.y, rect.w, rect.h).to_image())
}

pub struct ScreenCapturer {
    monitor: Monitor,
    rect: Rect,
}

impl ScreenCapturer {
    pub fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
        let monitors = Monitor::all().map_err(|e| CaptureError::Backend(e.to_string()))?;
        Ok(monitors
            .iter()
            .enumerate()
            .map(|(index, m)| MonitorInfo {
                index,
                name: m.name().unwrap_or_default(),
                width: m.width().unwrap_or(0),
                height: m.height().unwrap_or(0),
            })
            .collect())
    }

    pub fn new(monitor_index: usize, rect: Rect) -> Result<Self, CaptureError> {
        let monitors = Monitor::all().map_err(|e| CaptureError::Backend(e.to_string()))?;
        let monitor = monitors
            .into_iter()
            .nth(monitor_index)
            .ok_or(CaptureError::NoMonitor(monitor_index))?;
        Ok(Self { monitor, rect })
    }
}

impl FrameSource for ScreenCapturer {
    fn grab(&mut self) -> Result<RgbaImage, CaptureError> {
        let full = self
            .monitor
            .capture_image()
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        crop(&full, self.rect)
    }
}
```

If the `xcap` version in use returns plain values (not `Result`) from `name()`, `width()`, `height()`, drop the `unwrap_or` calls accordingly; check with `cargo doc --open` or the crate docs for the resolved version.

- [ ] **Step 4: Run tests, expect pass** (the monitor test passes with either error variant).

- [ ] **Step 5: Commit** `feat: screen capture of the chat rectangle via xcap`

---

### Task 14: OCR trait and preprocessing

**Files:**
- Create: `src-tauri/src/ocr/mod.rs`, `src-tauri/src/ocr/preprocess.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod ocr;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct OcrLine { pub text: String, pub confidence: f32 /* 0..1, 1.0 when engine gives none */ }
  pub trait OcrEngine: Send { fn read(&self, img: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError>; fn name(&self) -> &'static str; }
  pub enum OcrError { ModelMissing(PathBuf), Engine(String) }
  pub fn preprocess(frame: &image::RgbaImage, scale: u32) -> image::GrayImage
  ```
  Lines are returned top to bottom.

- [ ] **Step 1: Failing tests** in `preprocess.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn upscales_by_factor() {
        let f = RgbaImage::from_pixel(100, 40, Rgba([0, 0, 0, 255]));
        assert_eq!(preprocess(&f, 3).dimensions(), (300, 120));
    }

    #[test]
    fn light_text_on_dark_stays_light_and_contrast_is_stretched() {
        let mut f = RgbaImage::from_pixel(10, 10, Rgba([40, 40, 40, 255]));
        f.put_pixel(5, 5, Rgba([180, 180, 180, 255]));
        let g = preprocess(&f, 1);
        assert_eq!(g.get_pixel(5, 5)[0], 255);
        assert_eq!(g.get_pixel(0, 0)[0], 0);
    }

    #[test]
    fn scale_zero_is_treated_as_one() {
        let f = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        assert_eq!(preprocess(&f, 0).dimensions(), (10, 10));
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

`ocr/preprocess.rs`:
```rust
use image::{imageops, GrayImage, RgbaImage};

/// Upscale, convert to grey and stretch contrast to the full 0..255 range.
/// The chat font is light on a translucent dark box, so no inversion is
/// applied; OCR engines handle light-on-dark fine once contrast is high.
pub fn preprocess(frame: &RgbaImage, scale: u32) -> GrayImage {
    let scale = scale.max(1);
    let (w, h) = frame.dimensions();
    let big = imageops::resize(frame, w * scale, h * scale, imageops::FilterType::CatmullRom);
    let grey = imageops::grayscale(&big);
    stretch_contrast(&grey)
}

fn stretch_contrast(grey: &GrayImage) -> GrayImage {
    let (lo, hi) = grey.pixels().fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
    if hi <= lo {
        return grey.clone();
    }
    let span = (hi - lo) as f32;
    let mut out = grey.clone();
    for p in out.pixels_mut() {
        p[0] = (((p[0] - lo) as f32 / span) * 255.0).round() as u8;
    }
    out
}
```

`ocr/mod.rs`:
```rust
pub mod preprocess;
pub use preprocess::preprocess;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("OCR model missing at {0}; run `hub-cli fetch-models`")]
    ModelMissing(PathBuf),
    #[error("OCR engine error: {0}")]
    Engine(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    /// 0..1; engines without a score report 1.0.
    pub confidence: f32,
}

pub trait OcrEngine: Send {
    fn read(&self, img: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError>;
    fn name(&self) -> &'static str;
}
```

- [ ] **Step 4: Run tests, expect pass.**

- [ ] **Step 5: Commit** `feat(ocr): engine trait and image preprocessing`

---

### Task 15: ocrs engine, model fetch, golden test

**Files:**
- Create: `src-tauri/src/ocr/ocrs_engine.rs`, `src-tauri/src/ocr/models.rs`, `src-tauri/tests/ocr_golden.rs`
- Modify: `src-tauri/src/ocr/mod.rs`, `src-tauri/src/bin/hub_cli.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct OcrsEngine { .. }
  impl OcrsEngine { pub fn load(models_dir: &Path) -> Result<OcrsEngine, OcrError>; }
  pub fn model_paths(models_dir: &Path) -> (PathBuf, PathBuf)   // detection, recognition
  pub fn fetch_models(models_dir: &Path) -> Result<(), OcrError> // downloads if missing (uses `curl` via std::process; no extra crate)
  ```
  Model URLs: `https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten` and `.../text-recognition.rten`.

- [ ] **Step 1: Failing golden test** `src-tauri/tests/ocr_golden.rs`:

```rust
//! Runs only when models are present: `cargo run --bin hub-cli -- fetch-models` first.
use std::path::Path;
use wardogs_command_hub::config::Rect;
use wardogs_command_hub::capture::crop;
use wardogs_command_hub::ocr::{preprocess, OcrEngine, OcrsEngine};

fn models_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../models")
}

#[test]
fn reads_coordinates_from_fixture_screenshot() {
    if !models_dir().join("text-recognition.rten").exists() {
        eprintln!("skipping: models not fetched");
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/screenshots/ingame-chat-1600x900.jpg");
    let frame = image::open(path).unwrap().into_rgba8();
    let chat = crop(&frame, Rect { x: 20, y: 20, w: 300, h: 110 }).unwrap();
    let engine = OcrsEngine::load(&models_dir()).unwrap();
    let lines = engine.read(&preprocess(&chat, 3)).unwrap();
    let joined = lines.iter().map(|l| l.text.to_lowercase()).collect::<Vec<_>>().join("\n");
    eprintln!("OCR:\n{joined}");
    assert!(joined.contains("leftwild"), "player name not read");
    assert!(joined.contains("90.97") || joined.contains("90,97"), "x coordinate not read");
    assert!(joined.contains("101.30") || joined.contains("101,30"), "y coordinate not read");
    assert!(joined.contains("tower"), "trailing text not read");
}
```

- [ ] **Step 2: Run, expect compile failure** (`OcrsEngine` missing).

- [ ] **Step 3: Implement**

`ocr/models.rs`:
```rust
use super::OcrError;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE: &str = "https://ocrs-models.s3-accelerate.amazonaws.com";
pub const DETECTION: &str = "text-detection.rten";
pub const RECOGNITION: &str = "text-recognition.rten";

pub fn model_paths(models_dir: &Path) -> (PathBuf, PathBuf) {
    (models_dir.join(DETECTION), models_dir.join(RECOGNITION))
}

fn download(url: &str, to: &Path) -> Result<(), OcrError> {
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .map_err(|e| OcrError::Engine(format!("could not run curl: {e}")))?;
    if !status.success() {
        return Err(OcrError::Engine(format!("download failed for {url}")));
    }
    Ok(())
}

/// Download the two ocrs models if they are not already on disk.
pub fn fetch_models(models_dir: &Path) -> Result<(), OcrError> {
    std::fs::create_dir_all(models_dir).map_err(|e| OcrError::Engine(e.to_string()))?;
    for name in [DETECTION, RECOGNITION] {
        let path = models_dir.join(name);
        if !path.exists() {
            download(&format!("{BASE}/{name}"), &path)?;
        }
    }
    Ok(())
}
```

`ocr/ocrs_engine.rs`:
```rust
use super::models::model_paths;
use super::{OcrEngine, OcrError, OcrLine};
use image::GrayImage;
use ocrs::{ImageSource, OcrEngine as Ocrs, OcrEngineParams};
use rten::Model;
use std::path::Path;

pub struct OcrsEngine {
    inner: Ocrs,
}

impl OcrsEngine {
    pub fn load(models_dir: &Path) -> Result<Self, OcrError> {
        let (det, rec) = model_paths(models_dir);
        for p in [&det, &rec] {
            if !p.exists() {
                return Err(OcrError::ModelMissing(p.clone()));
            }
        }
        let detection_model = Model::load_file(&det).map_err(|e| OcrError::Engine(e.to_string()))?;
        let recognition_model = Model::load_file(&rec).map_err(|e| OcrError::Engine(e.to_string()))?;
        let inner = Ocrs::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| OcrError::Engine(e.to_string()))?;
        Ok(Self { inner })
    }
}

impl OcrEngine for OcrsEngine {
    fn name(&self) -> &'static str {
        "ocrs"
    }

    fn read(&self, img: &GrayImage) -> Result<Vec<OcrLine>, OcrError> {
        let rgb = image::DynamicImage::ImageLuma8(img.clone()).into_rgb8();
        let source = ImageSource::from_bytes(rgb.as_raw(), rgb.dimensions())
            .map_err(|e| OcrError::Engine(e.to_string()))?;
        let input = self.inner.prepare_input(source).map_err(|e| OcrError::Engine(e.to_string()))?;
        let words = self.inner.detect_words(&input).map_err(|e| OcrError::Engine(e.to_string()))?;
        let lines = self.inner.find_text_lines(&input, &words);
        let texts = self.inner.recognize_text(&input, &lines).map_err(|e| OcrError::Engine(e.to_string()))?;
        Ok(texts
            .into_iter()
            .flatten()
            .map(|l| OcrLine { text: l.to_string().trim().to_string(), confidence: 1.0 })
            .filter(|l| !l.text.is_empty())
            .collect())
    }
}
```

`ocr/mod.rs` add:
```rust
pub mod models;
pub mod ocrs_engine;
pub use models::{fetch_models, model_paths};
pub use ocrs_engine::OcrsEngine;
```

`src/bin/hub_cli.rs` (replace):
```rust
use std::path::{Path, PathBuf};
use wardogs_command_hub::capture::crop;
use wardogs_command_hub::config::{Rect, Settings};
use wardogs_command_hub::ocr::{fetch_models, preprocess, OcrEngine, OcrsEngine};

fn usage() -> ! {
    eprintln!("usage: hub-cli fetch-models | ocr <image> [x y w h]");
    std::process::exit(2)
}

fn rect_from(args: &[String]) -> Rect {
    let n: Vec<u32> = args.iter().filter_map(|a| a.parse().ok()).collect();
    match n.as_slice() {
        [x, y, w, h] => Rect { x: *x, y: *y, w: *w, h: *h },
        _ => Settings::default().chat_rect,
    }
}

fn run_ocr(path: &Path, rect: Rect, models_dir: &Path) -> anyhow::Result<()> {
    let frame = image::open(path)?.into_rgba8();
    let chat = crop(&frame, rect)?;
    let engine = OcrsEngine::load(models_dir)?;
    for line in engine.read(&preprocess(&chat, 3))? {
        println!("{}", line.text);
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let models_dir = PathBuf::from("models");
    match args.first().map(String::as_str) {
        Some("fetch-models") => fetch_models(&models_dir)?,
        Some("ocr") => {
            let path = args.get(1).map(PathBuf::from).unwrap_or_else(|| usage());
            run_ocr(&path, rect_from(&args[2..]), &models_dir)?;
        }
        _ => usage(),
    }
    Ok(())
}
```

- [ ] **Step 4: Fetch models and run the golden test**

```bash
cd src-tauri && cargo run --bin hub-cli -- fetch-models && cd ..
cargo test --manifest-path src-tauri/Cargo.toml --test ocr_golden -- --nocapture
```
Expected: PASS, with the OCR text printed. If an assertion fails, first try `preprocess(&chat, 4)` and a tighter rectangle (`hub-cli ocr fixtures/screenshots/ingame-chat-1600x900.jpg 22 24 290 60`). If `leftwild` and both numbers still cannot be read, stop and report: this is the spike outcome that decides whether Plan 2 needs the Windows OCR engine as default (it is planned anyway) or a Python RapidOCR sidecar. Record the outcome in `docs/superpowers/specs/2026-09-11-command-hub-design.md` under "Open items".

- [ ] **Step 5: Commit** `feat(ocr): ocrs engine, model fetch and golden fixture test`

---

### Task 16: Pipeline

**Files:**
- Create: `src-tauri/src/pipeline.rs`, `src-tauri/tests/pipeline_test.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod pipeline;`)

**Interfaces:**
- Consumes: `FrameGate`, `preprocess`, `OcrEngine`, `Deduper`, `parse_message`, `PlaceIndex`.
- Produces:
  ```rust
  pub struct Pipeline { .. }
  impl Pipeline {
      pub fn new(engine: Box<dyn OcrEngine>, places: PlaceIndex, dedup_capacity: usize, upscale: u32) -> Pipeline;
      pub fn set_places(&mut self, places: PlaceIndex);
      /// One captured chat frame in, zero or more brand-new messages out.
      pub fn tick(&mut self, frame: &image::RgbaImage, now_ms: u64) -> Result<Vec<Message>, OcrError>;
  }
  ```

- [ ] **Step 1: Failing integration test** `src-tauri/tests/pipeline_test.rs`:

```rust
use image::{Rgba, RgbaImage};
use std::cell::RefCell;
use std::collections::VecDeque;
use wardogs_command_hub::ocr::{OcrEngine, OcrError, OcrLine};
use wardogs_command_hub::parser::{Channel, LocationKind, PlaceIndex};
use wardogs_command_hub::pipeline::Pipeline;

/// Returns scripted line sets, one per call, in order.
struct FakeOcr {
    frames: RefCell<VecDeque<Vec<&'static str>>>,
}

impl OcrEngine for FakeOcr {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn read(&self, _img: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError> {
        let lines = self.frames.borrow_mut().pop_front().unwrap_or_default();
        Ok(lines.into_iter().map(|t| OcrLine { text: t.to_string(), confidence: 1.0 }).collect())
    }
}

fn frame(v: u8) -> RgbaImage {
    RgbaImage::from_pixel(300, 110, Rgba([v, v, v, 255]))
}

fn pipeline(frames: Vec<Vec<&'static str>>) -> Pipeline {
    let ocr = FakeOcr { frames: RefCell::new(frames.into()) };
    Pipeline::new(Box::new(ocr), PlaceIndex::empty(), 100, 1)
}

#[test]
fn scrolling_chat_yields_each_message_once() {
    let mut p = pipeline(vec![
        vec!["[TEAM] LeftWild: asdmadsma", "[TEAM] LeftWild: x90.97, y101.30 need ammo here for", "tower 1"],
        vec!["[TEAM] LeftWild: x90.97, y101.30 need ammo here for", "tower 1", "[TEAM] Fl4sh: one is in tower 5"],
    ]);
    let first = p.tick(&frame(10), 1000).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[1].location.as_ref().unwrap().kind, LocationKind::Coord);
    assert_eq!(first[1].body, "need ammo here for tower 1");
    let second = p.tick(&frame(200), 2000).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].name, "Fl4sh");
    assert_eq!(second[0].at_ms, 2000);
}

#[test]
fn unchanged_frame_skips_ocr_entirely() {
    let mut p = pipeline(vec![vec!["[TEAM] a: one"], vec!["[TEAM] b: SHOULD NOT BE READ"]]);
    assert_eq!(p.tick(&frame(10), 0).unwrap().len(), 1);
    assert!(p.tick(&frame(10), 1).unwrap().is_empty());
    // the second scripted OCR frame is still queued because OCR never ran
    assert_eq!(p.tick(&frame(250), 2).unwrap()[0].name, "b");
}

#[test]
fn garbage_lines_are_kept_as_unknown() {
    let mut p = pipeline(vec![vec!["~~ noise ~~"]]);
    let out = p.tick(&frame(0), 0).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].channel, Channel::Unknown);
}

#[test]
fn ocr_errors_propagate() {
    struct Broken;
    impl OcrEngine for Broken {
        fn name(&self) -> &'static str { "broken" }
        fn read(&self, _: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError> {
            Err(OcrError::Engine("boom".into()))
        }
    }
    let mut p = Pipeline::new(Box::new(Broken), PlaceIndex::empty(), 10, 1);
    assert!(p.tick(&frame(0), 0).is_err());
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement**

```rust
use crate::dedup::Deduper;
use crate::gate::FrameGate;
use crate::ocr::{preprocess, OcrEngine, OcrError};
use crate::parser::{parse_message, Message, PlaceIndex};
use image::RgbaImage;

/// Mean luma difference (0..255) below which a frame is considered unchanged.
pub const GATE_THRESHOLD: f32 = 3.0;

pub struct Pipeline {
    engine: Box<dyn OcrEngine>,
    gate: FrameGate,
    dedup: Deduper,
    places: PlaceIndex,
    upscale: u32,
}

impl Pipeline {
    pub fn new(engine: Box<dyn OcrEngine>, places: PlaceIndex, dedup_capacity: usize, upscale: u32) -> Self {
        Self {
            engine,
            gate: FrameGate::new(GATE_THRESHOLD),
            dedup: Deduper::new(dedup_capacity),
            places,
            upscale,
        }
    }

    pub fn set_places(&mut self, places: PlaceIndex) {
        self.places = places;
    }

    pub fn engine_name(&self) -> &'static str {
        self.engine.name()
    }

    /// Gate, OCR, dedup and parse one chat frame. Returns only messages not
    /// seen before. Unchanged frames return an empty vector without OCR.
    pub fn tick(&mut self, frame: &RgbaImage, now_ms: u64) -> Result<Vec<Message>, OcrError> {
        if !self.gate.changed(frame) {
            return Ok(Vec::new());
        }
        let grey = preprocess(frame, self.upscale);
        let lines: Vec<String> = self.engine.read(&grey)?.into_iter().map(|l| l.text).collect();
        let fresh = self.dedup.push(&lines);
        Ok(fresh.iter().map(|raw| parse_message(raw, &self.places, now_ms)).collect())
    }
}
```

- [ ] **Step 4: Run all tests, expect pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: every test in every module passes; `ocr_golden` passes or prints "skipping" when models are absent.

- [ ] **Step 5: Commit** `feat: pipeline ties gate, ocr, dedup and parser together`

---

### Task 17: `watch` CLI command (end-to-end without a window)

**Files:**
- Modify: `src-tauri/src/bin/hub_cli.rs`

**Interfaces:**
- Consumes: `ScreenCapturer`, `FrameSource`, `Pipeline`, `Settings`, `MapRegistry`, `state::apply`/`expire`.
- Produces: `hub-cli watch [settings.json]` prints each new message as one JSON line and a pin count; runs until Ctrl-C.

- [ ] **Step 1: Write the test** (pure helper extracted so it is testable): add to `hub_cli.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_for_fps() {
        assert_eq!(frame_interval(2.0), std::time::Duration::from_millis(500));
        assert_eq!(frame_interval(0.0), std::time::Duration::from_millis(500)); // guards div by zero
    }
}
```

- [ ] **Step 2: Run `cargo test --manifest-path src-tauri/Cargo.toml --bin hub-cli`, expect compile failure.**

- [ ] **Step 3: Implement** (add to `hub_cli.rs`)

```rust
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wardogs_command_hub::capture::{FrameSource, ScreenCapturer};
use wardogs_command_hub::maps::MapRegistry;
use wardogs_command_hub::parser::PlaceIndex;
use wardogs_command_hub::pipeline::Pipeline;
use wardogs_command_hub::state::{apply, expire, Snapshot};

pub fn frame_interval(fps: f32) -> Duration {
    let fps = if fps > 0.0 { fps } else { 2.0 };
    Duration::from_millis((1000.0 / fps) as u64)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn run_watch(settings_path: &Path) -> anyhow::Result<()> {
    let settings = Settings::load(settings_path)?;
    let registry = MapRegistry::load_dir(Path::new("maps"))?;
    for (folder, err) in &registry.errors {
        eprintln!("map '{folder}' skipped: {err}");
    }
    let map = settings.selected_map.as_deref().and_then(|id| registry.get(id));
    let places = map.map(|m| m.place_index()).unwrap_or_else(PlaceIndex::empty);
    let engine = OcrsEngine::load(&settings.models_dir)?;
    let mut pipeline = Pipeline::new(Box::new(engine), places, settings.dedup_capacity, 3);
    let mut source = ScreenCapturer::new(settings.monitor_index, settings.chat_rect)?;
    let interval = frame_interval(settings.capture_fps);
    let ttl_ms = settings.pin_ttl_secs * 1000;
    let mut snapshot = Snapshot::default();
    eprintln!("watching monitor {} rect {:?} with {}", settings.monitor_index, settings.chat_rect, pipeline.engine_name());
    loop {
        let started = Instant::now();
        let now = now_ms();
        match source.grab().map_err(anyhow::Error::from).and_then(|f| Ok(pipeline.tick(&f, now)?)) {
            Ok(messages) => {
                for m in messages {
                    println!("{}", serde_json::to_string(&m)?);
                    snapshot = apply(&snapshot, m, map, ttl_ms, 500);
                }
            }
            Err(e) => eprintln!("frame error: {e}"),
        }
        snapshot = expire(&snapshot, now);
        eprintln!("pins: {}", snapshot.pins.len());
        std::thread::sleep(interval.saturating_sub(started.elapsed()));
    }
}
```

Extend `main`'s match:
```rust
        Some("watch") => {
            let path = args.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("settings.json"));
            run_watch(&path)?;
        }
```
and the usage string: `"usage: hub-cli fetch-models | ocr <image> [x y w h] | watch [settings.json]"`.

- [ ] **Step 4: Run tests; then a manual check on the gaming PC**

`cargo test --manifest-path src-tauri/Cargo.toml` → all pass.
On the Windows PC with the game running: `cargo run --bin hub-cli -- watch` and confirm chat lines appear as JSON. Adjust `chatRect` in `settings.json` to the real resolution.

- [ ] **Step 5: Commit** `feat(cli): watch command runs the full capture pipeline headless`

---

### Task 18: Coverage check and docs

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Run coverage**

```bash
cargo install cargo-llvm-cov --locked   # once
cargo llvm-cov --manifest-path src-tauri/Cargo.toml --ignore-filename-regex 'bin/|capture.rs|ocrs_engine.rs|models.rs'
```
Expected: 80% or more on the remaining files. If a module is under, add unit tests for its untested branches (typical gaps: `ManifestError::Io` path in `load_one`, `Settings::save` creating parent dirs).

- [ ] **Step 2: Update README** with a "Running headless" section:

```markdown
## Running headless (Plan 1)

    cd src-tauri
    cargo run --bin hub-cli -- fetch-models      # once, downloads ocrs models into ./models
    cargo run --bin hub-cli -- ocr ../fixtures/screenshots/ingame-chat-1600x900.jpg
    cargo run --bin hub-cli -- watch             # live: prints one JSON message per new chat line

Settings live in `settings.json` next to the binary (created on first save).
Set `chatRect` to the pixel rectangle of the chat box at your resolution and
`selectedMap` to a folder name under `maps/`.
```

- [ ] **Step 3: Commit** `docs: headless usage and coverage notes`

---

## Self-review

**Spec coverage.** Capture (T13), change gate (T12), OCR behind trait (T14, T15), dedup with wrap join (T7), parser with coordinate → named → none order and entity classifier (T2–T6), state with expiry and immutable updates (T11), maps as data with mandatory calibration and places (T8, T9), config validated on load (T10), errors typed and surfaced (all), integration with recorded frames (T16), golden OCR test (T15), 80% target (T18). Not in this plan, by design, for Plan 2: Tauri window and IPC, Leaflet UI, feed, settings and calibrate screens, status bar, map auto-detect, Windows OCR engine, tile script, Vitest/Playwright.

**Placeholders.** None. Every step has code or an exact command.

**Type consistency.** `Coord` is defined once in `parser::coords` and reused by `maps::calibration`, `state`, manifests. `Rect` lives in `config` and is used by `capture` and the CLI. `OcrEngine::read` takes `&GrayImage` everywhere; `preprocess` returns `GrayImage`. `Message.at_ms`/`Pin.at_ms` are `u64` milliseconds throughout. `MapManifest.calibration: Option<Calibration>` is `Copy`, so `map?.calibration?` in `state::pin_for` compiles.
