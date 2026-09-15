# Wardogs Command Hub — Design

**Date:** 2026-09-11
**Status:** approved design, pre-implementation

## Problem

WARDOGS team chat moves fast. Players call out enemy vehicles, structures
and infantry either as a Mark Coordinates line (`📍 x77.90, y70.60`) or as
free text referencing named places ("one is in tower 5"). Nothing keeps a
running picture of where those callouts land. The game exposes chat only on
screen: there is no log file and the server RCON API has no chat endpoint.

## Goal

A desktop app that lives on a second monitor, reads the in-game chat by
screen capture and OCR, and keeps an always-current tactical map of reported
enemy positions plus a feed of every chat line.

## Non-goals (v1)

- Writing anything back into the game or into wardogs.tech.
- Sharing state with other players. Single machine, single user.
- Reading Discord.
- Elevation, artillery maths, routing.

## Constraints

- Runs on Windows (primary) and Linux (best effort). Game and app on the
  same PC, two monitors.
- Must not affect game performance noticeably: capture a small rectangle,
  skip OCR on unchanged frames.
- New maps must be addable without code changes. Currently three maps:
  Bakurani, Ozeti, Zestafona.

## Chat format (observed)

```
[TEAM] Qsing: 📍 x77.90, y70.60
[TEAM] mad_max103: help conquering tower 1, it's pretty defenseless
[TEAM] [DOGA] mg_nd: supplies delivered to tower 5 but now dead. enemys in there
[TEAM] Fl4sh: one is in tower 5
```

- Channel tag in square brackets (`TEAM`; `SQUAD` and `ALL` assumed).
- Optional clan tag in square brackets before the name.
- Name terminated by `: `.
- Mark Coordinates body is a pin emoji (lost in OCR) followed by
  `x<float>, y<float>`. Range and origin are not documented and map sizes
  vary, so the numeric range differs per map. Per-map calibration is
  therefore mandatory (see Maps); the app never assumes a range.
- A Mark Coordinates line can carry trailing free text:
  `📍 x90.97, y101.30 need ammo here for tower 1`. Coordinates above 100
  have been observed, confirming the range is per-map.
- Lines wrap on screen; a wrapped continuation has no channel prefix.
- The chat box sits top-left, is small and semi-transparent over the scene,
  and disappears after a few seconds without new messages. Capture must
  therefore run continuously; every line is seen on several frames before
  it fades. OCR pre-processing (upscale, greyscale, contrast) is required.
- Fixtures: `fixtures/screenshots/ingame-chat-1600x900.jpg` and
  `fixtures/chat/sample-01.txt`.

## Architecture

Tauri v2 desktop app. Rust backend does capture, OCR, parsing and state.
TypeScript frontend renders the map, pins, feed and settings. Backend pushes
events to the frontend over Tauri IPC; frontend calls commands for settings
and manual actions.

```
capture ──► change gate ──► ocr ──► line dedup ──► parser ──► state ──► IPC ──► UI
   ▲                                                   ▲
   └── settings (rectangle, fps)                       └── maps (calibration, named places, keywords)
```

### Backend modules (`src-tauri/src/`)

| Module      | Responsibility                                                               |
|-------------|------------------------------------------------------------------------------|
| `capture`   | Grab the configured chat rectangle from the configured monitor via `xcap`. |
| `gate`      | Perceptual hash of the frame; skip downstream work if unchanged.             |
| `ocr`       | `trait OcrEngine { fn read(&self, img) -> Result<Vec<OcrLine>> }`. Impls: `OcrsEngine` (pure Rust `ocrs` crate, both OSes, no system deps), `WindowsOcr` (windows-rs, Windows only). Selected by config, default Windows on Windows. Tesseract was dropped because it needs C libraries that are hard to build on Windows. |
| `dedup`     | Normalise lines (case, whitespace, common OCR confusions) and drop lines already seen in a ring buffer of the last N. Handles wrapped continuations by joining a prefix-less line to the previous one. |
| `parser`    | Pure: `&str -> Option<Message>`. Extracts channel, clan, name, body. Body matchers in order: coordinate pair, named location, none. Entity classifier: keyword lists for vehicle / structure / infantry / other. |
| `maps`      | Loads `maps/<id>/map.json`. Calibration (two-point affine game→pixel), named locations, tile metadata. |
| `detect`    | Compares a downscaled full-screen capture with each map thumbnail (normalised cross-correlation). Emits a `MapSuggested` event when confident. Never switches on its own. |
| `state`     | Immutable snapshot of messages and pins. New snapshot per update. Expiry by age. |
| `config`    | Persisted settings: monitor, rectangle, fps, OCR engine, pin TTL, selected map. Validated on load with clear errors. |
| `commands`  | Tauri commands and event emission. Thin. |

Every module returns `Result` with a typed error. Errors are logged with
context and forwarded to the UI as `StatusEvent { level, text }`. Nothing is
swallowed.

### Frontend (`src/`)

TypeScript, no framework. Leaflet with a `L.CRS.Simple` map over local tiles.

- `map/`: tile layer, pin layer with type icons, age fade, hover card
  (reporter, raw line, age).
- `feed/`: scrolling list of all messages, located ones highlighted, click
  to centre map.
- `settings/`: monitor and rectangle picker (live preview of the capture),
  fps, OCR engine, pin TTL.
- `calibrate/`: click two points on the map, paste their in-game
  coordinates, save to `map.json`.
- `status/`: status bar showing capture rate, OCR latency, last error.

### Data

```ts
type Message = {
  id: string; at: number; channel: 'TEAM'|'SQUAD'|'ALL'|'UNKNOWN';
  clan?: string; name: string; body: string; raw: string;
  location?: { kind: 'coord'|'named'; x: number; y: number; label?: string };
  entity: 'vehicle'|'structure'|'infantry'|'other';
}
type Pin = { messageId: string; x: number; y: number; entity: Entity; at: number; expiresAt: number }
```

### Maps on disk

```
maps/
  bakurani/
    source.png          # not committed if licence unclear; documented in SOURCE.md
    map.json
    tiles/{z}/{x}/{y}.webp   # generated, gitignored
    thumb.png           # generated, for detect
  ozeti/ ...
  zestafona/ ...
scripts/tile-maps.mjs   # sharp-based pyramid cutter, 256px tiles
```

`map.json`:

```json
{
  "id": "bakurani", "name": "Bakurani", "sourceSize": [16384, 16384],
  "calibration": null,
  "_calibration_example": { "a": { "game": [12.5, 40.0], "px": [2048, 6553] }, "b": { "game": [88.0, 90.0], "px": [14418, 14745] } },
  "places": { "tower 5": [4120, 9930] },
  "aliases": { "t5": "tower 5" }
}
```

Adding a map: add folder, `map.json`, run the tiler. No code changes.

## Error handling

- Capture failure (monitor gone, permission): status bar error, retry with
  backoff, never crash.
- OCR engine unavailable: fall back to the other engine, tell the user.
- Unparseable line: kept in the feed as `UNKNOWN` channel, never dropped.
- Invalid `map.json`: map listed as unavailable with the validation error.

## Testing

- `parser`, `dedup`, `maps::calibration`, entity classifier: unit tests on
  fixture files of real chat lines (`fixtures/chat/*.txt`). Property tests
  for calibration round-trip.
- `ocr`: golden test against a native-resolution screenshot with expected
  lines; asserts per-line accuracy above a threshold.
- `detect`: fixture screenshots of each map's tactical view.
- Integration: feed a recorded sequence of frames through the pipeline and
  assert the resulting state.
- Frontend: Vitest for pin fade and feed logic; one Playwright smoke test.
- Target 80% coverage on backend logic modules.

## Open items resolved by the first plan tasks

1. **OCR spike (throwaway):** native screenshot, compare Windows OCR vs
   `ocrs` on ~20 lines. Decides default engine. If both are poor, revisit
   with RapidOCR in a Python sidecar.

   **Result (Task 15, 2026-09-15):** `ocrs` 0.10.4 reads the fixture
   (`fixtures/screenshots/ingame-chat-1600x900.jpg`, chat rect
   `20,20,300,110`) almost completely: player name (`leftwild`), the x
   coordinate (`90.97`), and the trailing text (`tower`) all come through
   correctly. The y coordinate consistently drops its decimal point —
   `101.30` is read as `10130` — across every preprocessing scale tried (2,
   3, 4, 5) and every crop tried (full chat rect, and the tighter
   `22,24,290,60`). The missing period appears to be an `ocrs` recognition
   limitation on this small glyph rather than a preprocessing/crop problem,
   since it reproduces identically regardless of scale or rect. The golden
   test in `src-tauri/tests/ocr_golden.rs` is committed as originally
   specified (not weakened) and currently fails on this one assertion. This
   keeps Windows OCR as the planned default (already the plan) and leaves
   the door open to a heuristic fix downstream (e.g. re-inserting a decimal
   point into a 5-digit coordinate token) or a RapidOCR sidecar if `ocrs`
   proves insufficient on real (non-fixture) captures too.
2. **Coordinate range:** unknown and per-map. Each map is calibrated on a
   live match via the calibrate screen. An uncalibrated map shows located
   messages in the feed with a "calibrate to place" hint, and no pins.

## Repo

Public GitHub repo `wardogs-command-hub`, MIT licence, this spec committed
under `docs/superpowers/specs/`.
