use image::{imageops, GrayImage, RgbaImage};

/// Upscale, convert to grey and stretch contrast to the full 0..255 range.
/// The chat font is light on a translucent dark box, so no inversion is
/// applied; OCR engines handle light-on-dark fine once contrast is high.
pub fn preprocess(frame: &RgbaImage, scale: u32) -> GrayImage {
    let scale = scale.max(1);
    let (w, h) = frame.dimensions();
    let big = imageops::resize(
        frame,
        w * scale,
        h * scale,
        imageops::FilterType::CatmullRom,
    );
    let grey = imageops::grayscale(&big);
    stretch_contrast(&grey)
}

fn stretch_contrast(grey: &GrayImage) -> GrayImage {
    let (lo, hi) = grey
        .pixels()
        .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
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
