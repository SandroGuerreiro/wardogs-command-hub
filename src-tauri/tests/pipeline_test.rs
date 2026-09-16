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
        Ok(lines
            .into_iter()
            .map(|t| OcrLine {
                text: t.to_string(),
                confidence: 1.0,
            })
            .collect())
    }
}

fn frame(v: u8) -> RgbaImage {
    RgbaImage::from_pixel(300, 110, Rgba([v, v, v, 255]))
}

fn pipeline(frames: Vec<Vec<&'static str>>) -> Pipeline {
    let ocr = FakeOcr {
        frames: RefCell::new(frames.into()),
    };
    Pipeline::new(Box::new(ocr), PlaceIndex::empty(), 100, 1)
}

#[test]
fn scrolling_chat_yields_each_message_once() {
    let mut p = pipeline(vec![
        vec![
            "[TEAM] LeftWild: asdmadsma",
            "[TEAM] LeftWild: x90.97, y101.30 need ammo here for",
            "tower 1",
        ],
        vec![
            "[TEAM] LeftWild: x90.97, y101.30 need ammo here for",
            "tower 1",
            "[TEAM] Fl4sh: one is in tower 5",
        ],
    ]);
    let first = p.tick(&frame(10), 1000).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(
        first[1].location.as_ref().unwrap().kind,
        LocationKind::Coord
    );
    assert_eq!(first[1].body, "need ammo here for tower 1");
    let second = p.tick(&frame(200), 2000).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].name, "Fl4sh");
    assert_eq!(second[0].at_ms, 2000);
}

#[test]
fn unchanged_frame_skips_ocr_entirely() {
    let mut p = pipeline(vec![
        vec!["[TEAM] a: one"],
        vec!["[TEAM] b: SHOULD NOT BE READ"],
    ]);
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
        fn name(&self) -> &'static str {
            "broken"
        }
        fn read(&self, _: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError> {
            Err(OcrError::Engine("boom".into()))
        }
    }
    let mut p = Pipeline::new(Box::new(Broken), PlaceIndex::empty(), 10, 1);
    assert!(p.tick(&frame(0), 0).is_err());
}

/// Errors on the first call, then returns scripted lines on every later call.
struct FlakyOnceThenGood {
    lines: RefCell<Vec<&'static str>>,
    failed_once: RefCell<bool>,
}

impl OcrEngine for FlakyOnceThenGood {
    fn name(&self) -> &'static str {
        "flaky"
    }
    fn read(&self, _img: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError> {
        if !*self.failed_once.borrow() {
            *self.failed_once.borrow_mut() = true;
            return Err(OcrError::Engine("transient".into()));
        }
        Ok(self
            .lines
            .borrow()
            .iter()
            .map(|t| OcrLine {
                text: t.to_string(),
                confidence: 1.0,
            })
            .collect())
    }
}

#[test]
fn failed_read_does_not_commit_gate_so_retry_on_same_frame_still_reads() {
    let ocr = FlakyOnceThenGood {
        lines: RefCell::new(vec!["[TEAM] a: hello"]),
        failed_once: RefCell::new(false),
    };
    let mut p = Pipeline::new(Box::new(ocr), PlaceIndex::empty(), 10, 1);
    let f = frame(10);
    assert!(p.tick(&f, 0).is_err());
    // Same frame again: since the gate reference was never committed after
    // the failed read, this must still be treated as "changed" and OCR'd.
    let out = p.tick(&f, 1).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "a");
}
