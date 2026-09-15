//! Fetches the two `ocrs` model files used for text detection and recognition.

use super::OcrError;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE: &str = "https://ocrs-models.s3-accelerate.amazonaws.com";
pub const DETECTION: &str = "text-detection.rten";
pub const RECOGNITION: &str = "text-recognition.rten";

/// Paths to the detection and recognition model files under `models_dir`.
pub fn model_paths(models_dir: &Path) -> (PathBuf, PathBuf) {
    (models_dir.join(DETECTION), models_dir.join(RECOGNITION))
}

fn download(url: &str, to: &Path) -> Result<(), OcrError> {
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .map_err(|e| OcrError::Engine(format!("could not run curl: {e}")))?;
    if !status.success() {
        return Err(OcrError::Engine(format!("download failed for {url}")));
    }
    Ok(())
}

/// Download the two ocrs models into `models_dir` if they are not already present.
pub fn fetch_models(models_dir: &Path) -> Result<(), OcrError> {
    std::fs::create_dir_all(models_dir).map_err(|e| OcrError::Engine(e.to_string()))?;
    for name in [DETECTION, RECOGNITION] {
        let path = models_dir.join(name);
        if !path.exists() {
            download(&format!("{BASE}/{name}"), &path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_join_names_under_dir() {
        let dir = Path::new("models");
        let (det, rec) = model_paths(dir);
        assert_eq!(det, dir.join("text-detection.rten"));
        assert_eq!(rec, dir.join("text-recognition.rten"));
    }
}
