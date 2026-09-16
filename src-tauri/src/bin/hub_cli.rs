use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wardogs_command_hub::capture::{crop, FrameSource, ScreenCapturer};
use wardogs_command_hub::config::{OcrEngineKind, Rect, Settings};
use wardogs_command_hub::maps::MapRegistry;
use wardogs_command_hub::ocr::{fetch_models, preprocess, OcrEngine, OcrsEngine};
use wardogs_command_hub::parser::PlaceIndex;
use wardogs_command_hub::pipeline::Pipeline;
use wardogs_command_hub::state::{apply, expire, Snapshot};

/// Used both for the `ocr` command's preprocessing scale and the pipeline's
/// live upscale factor; the two never need to differ in practice.
const DEFAULT_SCALE: u32 = 3;
const DEFAULT_FPS: f32 = 2.0;
const MAX_MESSAGES: usize = 500;
const MAX_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF_EXPONENT: u32 = 16;

fn usage() -> ! {
    eprintln!("usage: hub-cli fetch-models | ocr <image> [x y w h] | watch [settings.json]");
    eprintln!(
        "  models/, maps/ and settings.json are resolved relative to the current working directory"
    );
    std::process::exit(2)
}

/// Parses optional `[x y w h]` rect args. `None` means no args were given
/// (caller should use a default); `Err` means args were given but did not
/// form exactly four `u32` values.
fn parse_rect(args: &[String]) -> Result<Option<Rect>, String> {
    if args.is_empty() {
        return Ok(None);
    }
    if args.len() != 4 {
        return Err(format!(
            "expected 4 rect values (x y w h), got {}",
            args.len()
        ));
    }
    let mut n = [0u32; 4];
    for (slot, a) in n.iter_mut().zip(args) {
        *slot = a
            .parse()
            .map_err(|_| format!("invalid rect value: '{a}'"))?;
    }
    Ok(Some(Rect {
        x: n[0],
        y: n[1],
        w: n[2],
        h: n[3],
    }))
}

fn rect_from(args: &[String]) -> Rect {
    match parse_rect(args) {
        Ok(Some(rect)) => rect,
        Ok(None) => Settings::default().chat_rect,
        Err(e) => {
            eprintln!("{e}");
            usage()
        }
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

/// Notice to print when `kind` names an OCR engine not available in this
/// build. This build only ships the `ocrs` engine, so any other requested
/// kind falls back to it. Returns `None` when no notice is needed.
fn engine_notice(kind: OcrEngineKind) -> Option<String> {
    match kind {
        OcrEngineKind::Ocrs => None,
        OcrEngineKind::Windows => Some(
            "requested OCR engine 'windows' is not available in this build; using ocrs instead"
                .to_string(),
        ),
    }
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

/// Backoff after `consecutive_errors` failed ticks: `base` doubled per error,
/// capped at `MAX_BACKOFF`. The exponent is capped before shifting so a huge
/// error count can't overflow.
pub fn backoff_for(base: Duration, consecutive_errors: u32) -> Duration {
    if consecutive_errors == 0 {
        return base;
    }
    let exponent = consecutive_errors.min(MAX_BACKOFF_EXPONENT);
    let factor = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
    base.saturating_mul(factor as u32).min(MAX_BACKOFF)
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
    if let Some(notice) = engine_notice(settings.ocr_engine) {
        eprintln!("{notice}");
    }
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
        DEFAULT_SCALE,
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

/// Prints `frame error: {e}` only when the message text differs from
/// `last_error`, then stores it. Returns the updated `last_error`.
fn log_error_once(e: &anyhow::Error, last_error: Option<String>) -> Option<String> {
    let text = e.to_string();
    if last_error.as_deref() != Some(text.as_str()) {
        eprintln!("frame error: {text}");
    }
    Some(text)
}

fn watch_loop(ctx: &mut WatchContext) -> anyhow::Result<()> {
    let mut snapshot = Snapshot::default();
    let mut consecutive_errors: u32 = 0;
    let mut last_error: Option<String> = None;
    let mut last_pins_printed: Option<usize> = None;
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
                consecutive_errors = 0;
                last_error = None;
                for m in messages {
                    println!("{}", serde_json::to_string(&m)?);
                    snapshot = apply(&snapshot, m, ctx.map.as_ref(), ctx.ttl_ms, MAX_MESSAGES);
                }
                snapshot = expire(&snapshot, now);
                print_pins_if_changed(snapshot.pins.len(), &mut last_pins_printed);
            }
            Err(e) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                last_error = log_error_once(&e, last_error);
                // Pins still need to age out during a capture outage, even
                // though no new messages arrived this tick.
                snapshot = expire(&snapshot, now);
                print_pins_if_changed(snapshot.pins.len(), &mut last_pins_printed);
            }
        }
        let sleep_for = backoff_for(ctx.interval, consecutive_errors);
        std::thread::sleep(sleep_for.saturating_sub(started.elapsed()));
    }
}

/// Prints `pins: N` only when `n` differs from the last printed count.
fn print_pins_if_changed(n: usize, last_printed: &mut Option<usize>) {
    if *last_printed != Some(n) {
        eprintln!("pins: {n}");
        *last_printed = Some(n);
    }
}

fn run_watch(settings_path: &Path) -> anyhow::Result<()> {
    let mut ctx = setup_watch(settings_path)?;
    watch_loop(&mut ctx)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Loaded once so `fetch-models`, `ocr` and `watch` all resolve models
    // from the same `settings.json`-configured directory (defaults when the
    // file is missing).
    let settings = Settings::load(Path::new("settings.json"))?;
    let models_dir = settings.models_dir.clone();
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

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_millis(500);
        assert_eq!(backoff_for(base, 0), Duration::from_millis(500));
        assert_eq!(backoff_for(base, 1), Duration::from_secs(1));
        assert_eq!(backoff_for(base, 3), Duration::from_secs(4));
        assert_eq!(backoff_for(base, 4), Duration::from_secs(5));
        assert_eq!(backoff_for(base, u32::MAX), Duration::from_secs(5));
    }

    #[test]
    fn engine_notice_for_unavailable_engine() {
        assert!(engine_notice(OcrEngineKind::Windows).is_some());
        assert_eq!(engine_notice(OcrEngineKind::Ocrs), None);
    }

    #[test]
    fn parse_rect_empty_is_default() {
        assert_eq!(parse_rect(&[]).unwrap(), None);
    }

    #[test]
    fn parse_rect_four_good_values() {
        let args: Vec<String> = ["1", "2", "3", "4"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            parse_rect(&args).unwrap(),
            Some(Rect {
                x: 1,
                y: 2,
                w: 3,
                h: 4
            })
        );
    }

    #[test]
    fn parse_rect_wrong_count_is_error() {
        let args: Vec<String> = ["1", "2", "3"].iter().map(|s| s.to_string()).collect();
        assert!(parse_rect(&args).is_err());
    }

    #[test]
    fn parse_rect_non_numeric_is_error() {
        let args: Vec<String> = ["1", "2", "3", "x"].iter().map(|s| s.to_string()).collect();
        assert!(parse_rect(&args).is_err());
    }

    #[test]
    fn pins_only_printed_when_changed() {
        let mut last = None;
        // First call always "changes" from None.
        print_pins_if_changed(0, &mut last);
        assert_eq!(last, Some(0));
        print_pins_if_changed(0, &mut last);
        assert_eq!(last, Some(0));
        print_pins_if_changed(3, &mut last);
        assert_eq!(last, Some(3));
    }
}
