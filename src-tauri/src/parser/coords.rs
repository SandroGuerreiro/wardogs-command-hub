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
        Regex::new(r"(?i)\bx\s*(\d+(?:[.,]\d+)?)\s*[,;]?\s*[yv]\s*(\d+(?:[.,]\d+)?)")
            .expect("static regex")
    })
}

/// OCR sometimes drops the small decimal-point glyph between a coordinate's
/// whole and fractional parts (`"101.30"` is read as `"10130"`). The game
/// always prints coordinates with exactly two decimal digits, so a
/// digit-only string of 3+ digits can be repaired by reinserting the point
/// before the last two digits. Strings that already have a separator, or
/// are too short to have a fractional part (e.g. plain integers like `"7"`
/// or `"12"`), are returned unchanged.
fn repair_decimal(s: &str) -> String {
    if s.contains('.') || s.contains(',') || s.len() < 3 {
        return s.to_string();
    }
    let split = s.len() - 2;
    format!("{}.{}", &s[..split], &s[split..])
}

fn parse_num(s: &str) -> Option<f64> {
    repair_decimal(s).replace(',', ".").parse().ok()
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
        assert_eq!(
            m.coord,
            Coord {
                x: 90.97,
                y: 101.30
            }
        );
        assert_eq!(m.rest, "need ammo here for tower 1");
    }

    #[test]
    fn ocr_variants() {
        assert!(find_coords("X 12.5 , Y 7").is_some());
        assert!(find_coords("x12,5; y7,25").is_some()); // decimal comma
        assert_eq!(
            find_coords("x12,5; y7,25").unwrap().coord,
            Coord { x: 12.5, y: 7.25 }
        );
    }

    #[test]
    fn no_pair() {
        assert!(find_coords("one is in tower 5").is_none());
        assert!(find_coords("x is fine").is_none());
    }

    #[test]
    fn ocr_dropped_decimal_is_repaired() {
        let m = find_coords("x9097 y10130").unwrap();
        assert_eq!(
            m.coord,
            Coord {
                x: 90.97,
                y: 101.30
            }
        );
    }

    #[test]
    fn v_is_accepted_for_y() {
        let m = find_coords("x90.97 v10130 need ammo").unwrap();
        assert_eq!(
            m.coord,
            Coord {
                x: 90.97,
                y: 101.30
            }
        );
        assert_eq!(m.rest, "need ammo");
    }

    #[test]
    fn short_integers_are_not_rescaled() {
        let m = find_coords("x5, y12").unwrap();
        assert_eq!(m.coord, Coord { x: 5.0, y: 12.0 });
    }
}
