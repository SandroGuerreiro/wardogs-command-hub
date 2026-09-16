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
            .map(|(_, name, coord)| PlaceMatch {
                name: name.clone(),
                coord: *coord,
            })
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
