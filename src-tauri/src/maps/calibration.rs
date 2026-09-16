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
    #[error(
        "calibration points share the same {0} value; pick two points that differ on both axes"
    )]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Coord;
    use proptest::prelude::*;

    fn cal() -> Calibration {
        Calibration {
            a: CalPoint {
                game: Coord { x: 0.0, y: 0.0 },
                px: Coord { x: 100.0, y: 200.0 },
            },
            b: CalPoint {
                game: Coord { x: 100.0, y: 50.0 },
                px: Coord {
                    x: 1100.0,
                    y: 700.0,
                },
            },
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
        assert_eq!(
            c.to_pixel(Coord { x: 50.0, y: 25.0 }),
            Coord { x: 600.0, y: 450.0 }
        );
        assert_eq!(
            c.to_pixel(Coord { x: 200.0, y: -50.0 }),
            Coord {
                x: 2100.0,
                y: -300.0
            }
        );
    }

    #[test]
    fn y_axis_may_be_flipped() {
        let c = Calibration {
            a: CalPoint {
                game: Coord { x: 0.0, y: 0.0 },
                px: Coord { x: 0.0, y: 1000.0 },
            },
            b: CalPoint {
                game: Coord { x: 10.0, y: 10.0 },
                px: Coord { x: 1000.0, y: 0.0 },
            },
        };
        assert_eq!(
            c.to_pixel(Coord { x: 5.0, y: 5.0 }),
            Coord { x: 500.0, y: 500.0 }
        );
        assert_eq!(
            c.to_pixel(Coord { x: 0.0, y: 10.0 }),
            Coord { x: 0.0, y: 0.0 }
        );
    }

    #[test]
    fn degenerate_points_rejected() {
        let c = Calibration {
            a: cal().a,
            b: cal().a,
        };
        assert!(matches!(
            c.validate(),
            Err(CalibrationError::DegenerateAxis(_))
        ));
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
