use crate::dedup::Deduper;
use crate::gate::FrameGate;
use crate::ocr::{preprocess, OcrEngine, OcrError};
use crate::parser::{parse_header, parse_message, Message, PlaceIndex};
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
    pub fn new(
        engine: Box<dyn OcrEngine>,
        places: PlaceIndex,
        dedup_capacity: usize,
        upscale: u32,
    ) -> Self {
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
        let lines: Vec<String> = self
            .engine
            .read(&grey)?
            .into_iter()
            .map(|l| l.text)
            .collect();
        let fresh = self.fresh_lines(&lines);
        Ok(fresh
            .iter()
            .map(|raw| parse_message(raw, &self.places, now_ms))
            .collect())
    }

    /// Runs cross-frame dedup for headed chat lines. A frame with no headed
    /// line at all (pure OCR noise, no `[CHANNEL] name:` anywhere) has no
    /// stable identity for `Deduper`'s continuation-joining to key off, so
    /// it always gets dropped as an orphan continuation. Surface it as-is
    /// instead of silently losing it.
    fn fresh_lines(&mut self, lines: &[String]) -> Vec<String> {
        if lines.iter().any(|l| parse_header(l).is_some()) {
            self.dedup.push(lines)
        } else {
            lines.to_vec()
        }
    }
}
