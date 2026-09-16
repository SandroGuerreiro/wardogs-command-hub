//! Fetches the two `ocrs` model files used for text detection and recognition.

use super::OcrError;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE: &str = "https://ocrs-models.s3-accelerate.amazonaws.com";
pub const DETECTION: &str = "text-detection.rten";
pub const RECOGNITION: &str = "text-recognition.rten";

/// Pinned SHA-256 digests for the two model files, computed once from a
/// known-good download. A mismatch means the download is corrupt, was
/// tampered with in transit, or the upstream file changed underneath us.
const DETECTION_SHA256: &str = "f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca";
const RECOGNITION_SHA256: &str = "e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e";

/// Expected SHA-256 digest for a model file by name, if known.
fn expected_sha256(name: &str) -> Option<&'static str> {
    match name {
        DETECTION => Some(DETECTION_SHA256),
        RECOGNITION => Some(RECOGNITION_SHA256),
        _ => None,
    }
}

/// Verifies that the file at `path` hashes to `expected_hex` (a lowercase
/// hex SHA-256 digest). Returns `OcrError::Engine` on mismatch or I/O error.
pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<(), OcrError> {
    let mut file = std::fs::File::open(path).map_err(|e| OcrError::Engine(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| OcrError::Engine(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(OcrError::Engine(format!(
            "checksum mismatch for {}: expected {expected_hex}, got {actual}",
            path.display()
        )))
    }
}

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
/// a later `fetch_models` call to mistake for a complete model. When
/// `expected_sha256` is given, the `.part` file's digest is verified before
/// it is renamed onto `to`; on mismatch the `.part` file is deleted and an
/// error is returned instead.
fn download(url: &str, to: &Path, expected_sha256: Option<&str>) -> Result<(), OcrError> {
    let part = part_path(to);
    remove_if_exists(&part)?;

    let result = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "-o",
        ])
        .arg(&part)
        .arg(url)
        .status()
        .map_err(|e| OcrError::Engine(format!("could not run curl: {e}")));

    match result {
        Ok(status) if status.success() => {
            if let Some(expected) = expected_sha256 {
                if let Err(e) = verify_sha256(&part, expected) {
                    remove_if_exists(&part)?;
                    let name = to.file_name().and_then(|n| n.to_str()).unwrap_or("model");
                    return Err(OcrError::Engine(format!(
                        "checksum mismatch for {name}: {e}"
                    )));
                }
            }
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
            download(&format!("{BASE}/{name}"), &path, expected_sha256(name))?;
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

        let result = download("http://127.0.0.1:9/nope", &target, None);

        assert!(result.is_err());
        assert!(!target.exists());
        assert!(!part_path(&target).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn checksum_matches_known_good_content() {
        let dir = std::env::temp_dir().join(format!("hub-ocr-sha-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known.bin");
        std::fs::write(&path, b"hello world").unwrap();
        // sha256("hello world")
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

        assert!(verify_sha256(&path, expected).is_ok());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let dir = std::env::temp_dir().join(format!("hub-ocr-sha-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wrong.bin");
        std::fs::write(&path, b"not the expected content").unwrap();

        let result = verify_sha256(
            &path,
            "0000000000000000000000000000000000000000000000000000000000000000",
        );

        assert!(matches!(result, Err(OcrError::Engine(_))));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
