use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wardogs_command_hub::capture::{crop, FrameSource, ScreenCapturer};
use wardogs_command_hub::config::{Rect, Settings};
use wardogs_command_hub::maps::MapRegistry;
use wardogs_command_hub::ocr::{fetch_models, preprocess, OcrEngine, OcrsEngine};
use wardogs_command_hub::parser::PlaceIndex;
use wardogs_command_hub::pipeline::Pipeline;
use wardogs_command_hub::state::{apply, expire, Snapshot};

const DEFAULT_SCALE: u32 = 3;
const DEFAULT_FPS: f32 = 2.0;
const DEFAULT_UPSCALE: u32 = 3;
const MAX_MESSAGES: usize = 500;

fn usage() -> ! {
    eprintln!("usage: hub-cli fetch-models | ocr <image> [x y w h] | watch [settings.json]");
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

/// Interval between captures for a given fps. Non-positive fps falls back to
/// `DEFAULT_FPS` rather than dividing by zero.
pub fn frame_interval(fps: f32) -> Duration {
    let fps = if fps > 0.0 { fps } else { DEFAULT_FPS };
    Duration::from_millis((1000.0 / fps) as u64)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pieces needed to drive the watch loop, built once from settings/maps.
struct WatchContext {
    pipeline: Pipeline,
    source: ScreenCapturer,
    interval: Duration,
    ttl_ms: u64,
    map: Option<wardogs_command_hub::maps::MapManifest>,
}

fn setup_watch(settings_path: &Path) -> anyhow::Result<WatchContext> {
    let settings = Settings::load(settings_path)?;
    let registry = MapRegistry::load_dir(Path::new("maps"))?;
    for (folder, err) in &registry.errors {
        eprintln!("map '{folder}' skipped: {err}");
    }
    let map = settings
        .selected_map
        .as_deref()
        .and_then(|id| registry.get(id))
        .cloned();
    let places = map
        .as_ref()
        .map(|m| m.place_index())
        .unwrap_or_else(PlaceIndex::empty);
    let engine = OcrsEngine::load(&settings.models_dir)?;
    let pipeline = Pipeline::new(
        Box::new(engine),
        places,
        settings.dedup_capacity,
        DEFAULT_UPSCALE,
    );
    let source = ScreenCapturer::new(settings.monitor_index, settings.chat_rect)?;
    let interval = frame_interval(settings.capture_fps);
    let ttl_ms = settings.pin_ttl_secs * 1000;
    eprintln!(
        "watching monitor {} rect {:?} with {}",
        settings.monitor_index,
        settings.chat_rect,
        pipeline.engine_name()
    );
    Ok(WatchContext {
        pipeline,
        source,
        interval,
        ttl_ms,
        map,
    })
}

fn watch_loop(ctx: &mut WatchContext) -> anyhow::Result<()> {
    let mut snapshot = Snapshot::default();
    loop {
        let started = Instant::now();
        let now = now_ms();
        match ctx
            .source
            .grab()
            .map_err(anyhow::Error::from)
            .and_then(|f| ctx.pipeline.tick(&f, now).map_err(anyhow::Error::from))
        {
            Ok(messages) => {
                for m in messages {
                    println!("{}", serde_json::to_string(&m)?);
                    snapshot = apply(&snapshot, m, ctx.map.as_ref(), ctx.ttl_ms, MAX_MESSAGES);
                }
            }
            Err(e) => eprintln!("frame error: {e}"),
        }
        snapshot = expire(&snapshot, now);
        eprintln!("pins: {}", snapshot.pins.len());
        std::thread::sleep(ctx.interval.saturating_sub(started.elapsed()));
    }
}

fn run_watch(settings_path: &Path) -> anyhow::Result<()> {
    let mut ctx = setup_watch(settings_path)?;
    watch_loop(&mut ctx)
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
        Some("watch") => {
            let path = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("settings.json"));
            run_watch(&path)?;
        }
        _ => usage(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_for_fps() {
        assert_eq!(frame_interval(2.0), std::time::Duration::from_millis(500));
        assert_eq!(frame_interval(0.0), std::time::Duration::from_millis(500)); // guards div by zero
    }
}
