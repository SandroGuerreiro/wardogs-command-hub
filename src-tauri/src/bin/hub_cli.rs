use std::path::{Path, PathBuf};
use wardogs_command_hub::capture::crop;
use wardogs_command_hub::config::{Rect, Settings};
use wardogs_command_hub::ocr::{fetch_models, preprocess, OcrEngine, OcrsEngine};

const DEFAULT_SCALE: u32 = 3;

fn usage() -> ! {
    eprintln!("usage: hub-cli fetch-models | ocr <image> [x y w h]");
    std::process::exit(2)
}

fn rect_from(args: &[String]) -> Rect {
    let n: Vec<u32> = args.iter().filter_map(|a| a.parse().ok()).collect();
    match n.as_slice() {
        [x, y, w, h] => Rect {
            x: *x,
            y: *y,
            w: *w,
            h: *h,
        },
        _ => Settings::default().chat_rect,
    }
}

fn run_ocr(path: &Path, rect: Rect, models_dir: &Path) -> anyhow::Result<()> {
    let frame = image::open(path)?.into_rgba8();
    let chat = crop(&frame, rect)?;
    let engine = OcrsEngine::load(models_dir)?;
    for line in engine.read(&preprocess(&chat, DEFAULT_SCALE))? {
        println!("{}", line.text);
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let models_dir = PathBuf::from("models");
    match args.first().map(String::as_str) {
        Some("fetch-models") => fetch_models(&models_dir)?,
        Some("ocr") => {
            let path = args.get(1).map(PathBuf::from).unwrap_or_else(|| usage());
            run_ocr(&path, rect_from(&args[2..]), &models_dir)?;
        }
        _ => usage(),
    }
    Ok(())
}
