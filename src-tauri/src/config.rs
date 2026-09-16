use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings file is not valid JSON: {0}")]
    Json(String),
    #[error("settings invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OcrEngineKind {
    Ocrs,
    Windows,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub monitor_index: usize,
    pub chat_rect: Rect,
    pub capture_fps: f32,
    pub ocr_engine: OcrEngineKind,
    pub pin_ttl_secs: u64,
    pub selected_map: Option<String>,
    pub dedup_capacity: usize,
    pub models_dir: PathBuf,
}

pub const MAX_FPS: f32 = 60.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            monitor_index: 0,
            // matches the 1600x900 fixture: chat box top-left
            chat_rect: Rect {
                x: 20,
                y: 20,
                w: 300,
                h: 110,
            },
            capture_fps: 2.0,
            ocr_engine: if cfg!(windows) {
                OcrEngineKind::Windows
            } else {
                OcrEngineKind::Ocrs
            },
            pin_ttl_secs: 300,
            selected_map: None,
            dedup_capacity: 200,
            models_dir: PathBuf::from("models"),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let inv = |s: &str| Err(ConfigError::Invalid(s.to_string()));
        if self.chat_rect.w == 0 || self.chat_rect.h == 0 {
            return inv("chat rectangle must have non-zero width and height");
        }
        if !(self.capture_fps > 0.0 && self.capture_fps <= MAX_FPS) {
            return inv("capture fps must be between 0 and 60");
        }
        if self.pin_ttl_secs == 0 {
            return inv("pin ttl must be at least 1 second");
        }
        if self.dedup_capacity == 0 {
            return inv("dedup capacity must be at least 1");
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let s: Settings =
            serde_json::from_str(&text).map_err(|e| ConfigError::Json(e.to_string()))?;
        s.validate()?;
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| ConfigError::Json(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn rejects_zero_rect_and_silly_fps() {
        let s = Settings {
            chat_rect: Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 10,
            },
            ..Settings::default()
        };
        assert!(matches!(s.validate(), Err(ConfigError::Invalid(_))));
        let s = Settings {
            capture_fps: 0.0,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        let s = Settings {
            capture_fps: 61.0,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn round_trips_through_file() {
        let p = std::env::temp_dir().join(format!("hub-settings-{}.json", std::process::id()));
        let s = Settings {
            selected_map: Some("ozeti".into()),
            ..Settings::default()
        };
        s.save(&p).unwrap();
        let back = Settings::load(&p).unwrap();
        assert_eq!(back, s);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("hub-settings-dir-{}", std::process::id()));
        let p = dir.join("nested/settings.json");
        assert!(!dir.exists());
        Settings::default().save(&p).unwrap();
        assert!(p.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_rejects_invalid_settings_without_writing() {
        let p =
            std::env::temp_dir().join(format!("hub-settings-invalid-{}.json", std::process::id()));
        let s = Settings {
            pin_ttl_secs: 0,
            ..Settings::default()
        };
        assert!(matches!(s.save(&p), Err(ConfigError::Invalid(_))));
        assert!(!p.exists());
    }

    #[test]
    fn missing_file_yields_defaults() {
        let p = std::env::temp_dir().join("hub-settings-definitely-missing.json");
        assert_eq!(Settings::load(&p).unwrap(), Settings::default());
    }

    #[test]
    fn corrupt_file_is_an_error_not_defaults() {
        let p = std::env::temp_dir().join(format!("hub-settings-bad-{}.json", std::process::id()));
        std::fs::write(&p, "{ nope").unwrap();
        assert!(matches!(Settings::load(&p), Err(ConfigError::Json(_))));
        std::fs::remove_file(p).unwrap();
    }
}
