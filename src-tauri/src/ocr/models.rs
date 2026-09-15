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

/// Remove a file if it exists; a missing file is not an error.
fn remove_if_exists(path: &Path) -> Result<(), OcrError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(OcrError::Engine(e.to_string())),
    }
}

/// Path of the temporary sibling file a download is written to before being
/// renamed onto the real target.
fn part_path(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    to.with_file_name(name)
}

/// Download `url` to `to`, writing through a `.part` sibling file first so a
/// failed or interrupted download never leaves a truncated file at `to` for
/// a later `fetch_models` call to mistake for a complete model.
fn download(url: &str, to: &Path) -> Result<(), OcrError> {
    let part = part_path(to);
    remove_if_exists(&part)?;

    let result = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&part)
        .arg(url)
        .status()
        .map_err(|e| OcrError::Engine(format!("could not run curl: {e}")));

    match result {
        Ok(status) if status.success() => {
            std::fs::rename(&part, to).map_err(|e| OcrError::Engine(e.to_string()))
        }
        Ok(_) => {
            remove_if_exists(&part)?;
            Err(OcrError::Engine(format!("download failed for {url}")))
        }
        Err(e) => {
            remove_if_exists(&part)?;
            Err(e)
        }
    }
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

    #[test]
    fn failed_download_leaves_no_file() {
        let dir = std::env::temp_dir().join(format!("hub-ocr-download-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("x.rten");

        let result = download("http://127.0.0.1:9/nope", &target);

        assert!(result.is_err());
        assert!(!target.exists());
        assert!(!part_path(&target).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
