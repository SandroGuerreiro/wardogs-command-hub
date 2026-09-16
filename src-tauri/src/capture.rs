//! Screen capture of the game's chat rectangle via the `xcap` crate.

use crate::config::Rect;
use image::{imageops, RgbaImage};
use xcap::Monitor;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("monitor {0} not found")]
    NoMonitor(usize),
    #[error("chat rectangle {rect:?} exceeds frame {width}x{height}")]
    RectOutOfBounds { rect: Rect, width: u32, height: u32 },
    #[error("capture backend error: {0}")]
    Backend(String),
}

pub trait FrameSource {
    fn grab(&mut self) -> Result<RgbaImage, CaptureError>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

pub fn crop(frame: &RgbaImage, rect: Rect) -> Result<RgbaImage, CaptureError> {
    let (width, height) = frame.dimensions();
    let fits = rect.x.checked_add(rect.w).is_some_and(|r| r <= width)
        && rect.y.checked_add(rect.h).is_some_and(|b| b <= height);
    if !fits {
        return Err(CaptureError::RectOutOfBounds {
            rect,
            width,
            height,
        });
    }
    Ok(imageops::crop_imm(frame, rect.x, rect.y, rect.w, rect.h).to_image())
}

pub struct ScreenCapturer {
    monitor: Monitor,
    rect: Rect,
}

impl ScreenCapturer {
    pub fn list_monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
        let monitors = Monitor::all().map_err(|e| CaptureError::Backend(e.to_string()))?;
        Ok(monitors
            .iter()
            .enumerate()
            .map(|(index, m)| MonitorInfo {
                index,
                name: m.name().unwrap_or_default(),
                width: m.width().unwrap_or(0),
                height: m.height().unwrap_or(0),
            })
            .collect())
    }

    pub fn new(monitor_index: usize, rect: Rect) -> Result<Self, CaptureError> {
        let monitors = Monitor::all().map_err(|e| CaptureError::Backend(e.to_string()))?;
        let monitor = monitors
            .into_iter()
            .nth(monitor_index)
            .ok_or(CaptureError::NoMonitor(monitor_index))?;
        Ok(Self { monitor, rect })
    }
}

impl FrameSource for ScreenCapturer {
    fn grab(&mut self) -> Result<RgbaImage, CaptureError> {
        let full = self
            .monitor
            .capture_image()
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        crop(&full, self.rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn crop_returns_requested_size() {
        let f = RgbaImage::from_pixel(100, 50, Rgba([1, 2, 3, 255]));
        let c = crop(
            &f,
            Rect {
                x: 10,
                y: 5,
                w: 30,
                h: 20,
            },
        )
        .unwrap();
        assert_eq!(c.dimensions(), (30, 20));
    }

    #[test]
    fn crop_out_of_bounds_is_error() {
        let f = RgbaImage::from_pixel(100, 50, Rgba([0; 4]));
        let e = crop(
            &f,
            Rect {
                x: 90,
                y: 0,
                w: 30,
                h: 20,
            },
        )
        .unwrap_err();
        assert!(matches!(e, CaptureError::RectOutOfBounds { .. }));
    }

    #[test]
    fn bad_monitor_index_is_error_or_backend_error() {
        // On a headless CI there may be no monitors at all: either error is acceptable,
        // but it must not panic.
        let r = ScreenCapturer::new(
            usize::MAX,
            Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
        );
        assert!(r.is_err());
    }
}
