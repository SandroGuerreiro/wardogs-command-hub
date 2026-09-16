use crate::parser::parse_header;
use std::collections::VecDeque;

/// Lowercase, trim, collapse whitespace. Used only for equality, never shown.
pub fn normalise_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True when a line has fewer than 3 alphanumeric characters. The OCR engine
/// occasionally detects stray 1-2 character boxes on real frames (e.g. "a",
/// "n", "?"); these carry no useful signal and would otherwise be treated as
/// either orphan continuations or standalone headerless messages.
fn is_noise(line: &str) -> bool {
    line.chars().filter(|c| c.is_alphanumeric()).count() < 3
}

/// Joins wrapped continuation lines (no `[CHANNEL] name:` header) onto the
/// preceding headed line. If the frame has at least one headed line, a
/// leading continuation with no parent is dropped (it is presumed to be the
/// tail of an already-scrolled-off message). If the frame has no headed line
/// at all, every line is kept standalone — there is no header context to
/// join against, so each line is passed through the noise filter and kept
/// for normal dedup.
///
/// A short line that directly follows a headed line is exempt from the
/// noise filter: it is presumed to be a wrapped continuation fragment (e.g.
/// "5" wrapping from "...go to tower") rather than OCR noise. A short line
/// with no preceding headed line to join is still dropped either way, since
/// it has nothing to attach to.
fn join_wrapped(frame_lines: &[String]) -> Vec<String> {
    if frame_lines.iter().all(|l| parse_header(l).is_none()) {
        return frame_lines
            .iter()
            .filter(|l| !is_noise(l))
            .cloned()
            .collect();
    }
    frame_lines.iter().fold(Vec::new(), |mut acc, line| {
        let is_headed = parse_header(line).is_some();
        match (is_headed, acc.last_mut()) {
            (true, _) if !is_noise(line) => acc.push(line.trim().to_string()),
            (true, _) => {}
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
        Self {
            seen: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
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
        let out = d.push(&lines(&[
            "[TEAM] LeftWild: x90.97, y101.30 need ammo here for",
            "tower 1",
        ]));
        assert_eq!(
            out,
            lines(&["[TEAM] LeftWild: x90.97, y101.30 need ammo here for tower 1"])
        );
    }

    #[test]
    fn leading_continuation_without_parent_is_dropped() {
        // top of the box can show the tail of an already-scrolled line
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["tower 1", "[TEAM] a: b"]));
        assert_eq!(out, lines(&["[TEAM] a: b"]));
    }

    #[test]
    fn leading_standalone_noise_is_dropped() {
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["a", "[TEAM] x: hi"]));
        assert_eq!(out, lines(&["[TEAM] x: hi"]));
    }

    #[test]
    fn short_line_following_headed_line_is_a_continuation_not_noise() {
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["[TEAM] a: go to tower", "5"]));
        assert_eq!(out, lines(&["[TEAM] a: go to tower 5"]));

        let mut d2 = Deduper::new(50);
        let out2 = d2.push(&lines(&["a", "[TEAM] x: hi"]));
        assert_eq!(out2, lines(&["[TEAM] x: hi"]));
    }

    #[test]
    fn headerless_frame_lines_are_kept_and_deduped() {
        let mut d = Deduper::new(50);
        let out = d.push(&lines(&["~~ noise ~~"]));
        assert_eq!(out, lines(&["~~ noise ~~"]));
        assert!(d.push(&lines(&["~~ noise ~~"])).is_empty());
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
        assert_eq!(
            normalise_line("  [TEAM]  A:  Hi  There "),
            "[team] a: hi there"
        );
    }
}
