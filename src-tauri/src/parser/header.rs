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
        body: caps
            .get(4)
            .map(|m| m.as_str().trim_end())
            .unwrap_or("")
            .to_string(),
    })
}

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
        assert_eq!(
            parse_header("[SQUAD] a: b").unwrap().channel,
            Channel::Squad
        );
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
