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
    let sum: u32 = a
        .iter()
        .zip(b)
        .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs())
        .sum();
    sum as f32 / a.len().max(1) as f32
}

/// Maximum per-row mean absolute difference, one row at a time. A change
/// localized to a single thumbnail row (e.g. one new chat line among 8 rows
/// of thumbnail) can be diluted below threshold by the global mean; taking
/// the max across rows catches it instead.
fn max_row_mean_abs_diff(a: &[u8], b: &[u8], row_width: usize) -> f32 {
    a.chunks(row_width.max(1))
        .zip(b.chunks(row_width.max(1)))
        .map(|(ra, rb)| mean_abs_diff(ra, rb))
        .fold(0.0, f32::max)
}

/// Skips OCR when the chat rectangle has not visibly changed.
#[derive(Debug)]
pub struct FrameGate {
    last: Option<Vec<u8>>,
    threshold: f32,
}

impl FrameGate {
    pub fn new(threshold: f32) -> Self {
        Self {
            last: None,
            threshold,
        }
    }

    /// True when `frame` differs enough from the last *committed* reference
    /// to warrant OCR. Does not update the reference; call `commit` after a
    /// successful read so a failed OCR attempt doesn't silently adopt a
    /// changed-but-unprocessed frame as the new baseline.
    pub fn peek(&self, frame: &RgbaImage) -> bool {
        let now = thumb_luma(frame);
        match &self.last {
            None => true,
            Some(prev) => max_row_mean_abs_diff(prev, &now, THUMB_W as usize) >= self.threshold,
        }
    }

    /// Adopts `frame` as the new reference for future `peek`/`changed` calls.
    pub fn commit(&mut self, frame: &RgbaImage) {
        self.last = Some(thumb_luma(frame));
    }

    /// Convenience combining `peek` + `commit`: reports change and, if
    /// changed, immediately commits the new reference. Callers that need to
    /// gate committing on a downstream success (e.g. a successful OCR read)
    /// should use `peek`/`commit` directly instead.
    pub fn changed(&mut self, frame: &RgbaImage) -> bool {
        let changed = self.peek(frame);
        if changed {
            self.commit(frame);
        }
        changed
    }
}

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

    #[test]
    fn single_dim_text_row_is_detected() {
        // Reference: uniformly dark chat box. Candidate: identical except
        // one band of rows (a single new chat line) turns noticeably
        // lighter. The global mean dilutes this across all 8 thumbnail
        // rows, but the max per-row diff catches it.
        let mut g = FrameGate::new(3.0);
        let reference = RgbaImage::from_pixel(300, 110, Rgba([30, 30, 30, 255]));
        assert!(g.changed(&reference));

        let mut candidate = RgbaImage::from_pixel(300, 110, Rgba([30, 30, 30, 255]));
        for y in 0..14 {
            for x in 0..300 {
                candidate.put_pixel(x, y, Rgba([70, 70, 70, 255]));
            }
        }
        assert!(g.changed(&candidate));
    }
}
