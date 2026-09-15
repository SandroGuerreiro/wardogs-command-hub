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
