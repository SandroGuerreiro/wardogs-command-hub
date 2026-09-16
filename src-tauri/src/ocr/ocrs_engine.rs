//! `OcrEngine` implementation backed by the pure-Rust `ocrs` crate.

use super::models::model_paths;
use super::{OcrEngine, OcrError, OcrLine};
use image::GrayImage;
use ocrs::{ImageSource, OcrEngine as Ocrs, OcrEngineParams};
use rten::Model;
use std::path::Path;

pub struct OcrsEngine {
    inner: Ocrs,
}

impl OcrsEngine {
    /// Load the detection and recognition models from `models_dir`.
    pub fn load(models_dir: &Path) -> Result<Self, OcrError> {
        let (det, rec) = model_paths(models_dir);
        for p in [&det, &rec] {
            if !p.exists() {
                return Err(OcrError::ModelMissing(p.clone()));
            }
        }
        let detection_model =
            Model::load_file(&det).map_err(|e| OcrError::Engine(e.to_string()))?;
        let recognition_model =
            Model::load_file(&rec).map_err(|e| OcrError::Engine(e.to_string()))?;
        let inner = Ocrs::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| OcrError::Engine(e.to_string()))?;
        Ok(Self { inner })
    }
}

impl OcrEngine for OcrsEngine {
    fn name(&self) -> &'static str {
        "ocrs"
    }

    fn read(&self, img: &GrayImage) -> Result<Vec<OcrLine>, OcrError> {
        let rgb = image::DynamicImage::ImageLuma8(img.clone()).into_rgb8();
        let source = ImageSource::from_bytes(rgb.as_raw(), rgb.dimensions())
            .map_err(|e| OcrError::Engine(e.to_string()))?;
        let input = self
            .inner
            .prepare_input(source)
            .map_err(|e| OcrError::Engine(e.to_string()))?;
        let words = self
            .inner
            .detect_words(&input)
            .map_err(|e| OcrError::Engine(e.to_string()))?;
        let lines = self.inner.find_text_lines(&input, &words);
        let texts = self
            .inner
            .recognize_text(&input, &lines)
            .map_err(|e| OcrError::Engine(e.to_string()))?;
        Ok(texts
            .into_iter()
            .flatten()
            .map(|l| OcrLine {
                text: l.to_string().trim().to_string(),
                confidence: 1.0,
            })
            .filter(|l| !l.text.is_empty())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_models_yields_model_missing_error() {
        let dir = std::env::temp_dir().join(format!("hub-ocr-missing-{}", std::process::id()));
        let res = OcrsEngine::load(&dir);
        assert!(matches!(res, Err(OcrError::ModelMissing(_))));
    }
}
