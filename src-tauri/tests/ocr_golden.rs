//! Runs only when models are present: `cargo run --bin hub-cli -- fetch-models` first.
use std::path::Path;
use wardogs_command_hub::capture::crop;
use wardogs_command_hub::config::Rect;
use wardogs_command_hub::ocr::{preprocess, OcrEngine, OcrsEngine};

fn models_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../models")
}

#[test]
fn reads_coordinates_from_fixture_screenshot() {
    if !models_dir().join("text-recognition.rten").exists() {
        eprintln!("skipping: models not fetched");
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/screenshots/ingame-chat-1600x900.jpg");
    let frame = image::open(path).unwrap().into_rgba8();
    let chat = crop(
        &frame,
        Rect {
            x: 20,
            y: 20,
            w: 300,
            h: 110,
        },
    )
    .unwrap();
    let engine = OcrsEngine::load(&models_dir()).unwrap();
    let lines = engine.read(&preprocess(&chat, 3)).unwrap();
    let joined = lines
        .iter()
        .map(|l| l.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    eprintln!("OCR:\n{joined}");
    assert!(joined.contains("leftwild"), "player name not read");
    assert!(
        joined.contains("90.97") || joined.contains("90,97"),
        "x coordinate not read"
    );
    assert!(
        joined.contains("101.30") || joined.contains("101,30"),
        "y coordinate not read"
    );
    assert!(joined.contains("tower"), "trailing text not read");
}
