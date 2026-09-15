pub mod preprocess;
pub use preprocess::preprocess;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("OCR model missing at {0}; run `hub-cli fetch-models`")]
    ModelMissing(PathBuf),
    #[error("OCR engine error: {0}")]
    Engine(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    /// 0..1; engines without a score report 1.0.
    pub confidence: f32,
}

pub trait OcrEngine: Send {
    fn read(&self, img: &image::GrayImage) -> Result<Vec<OcrLine>, OcrError>;
    fn name(&self) -> &'static str;
}
