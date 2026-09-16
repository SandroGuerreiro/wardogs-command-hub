# Wardogs Command Hub

Second-monitor tactical map for WARDOGS. Reads in-game team chat via screen
capture and OCR, drops pins for Mark Coordinates callouts and named places,
and keeps a scrolling feed so nothing gets lost.

Fan-made. Not affiliated with Bulkhead or Team17.

See `docs/superpowers/specs/` for the design.

## Running headless (Plan 1)

Run these from the repo root:

    cargo run --manifest-path src-tauri/Cargo.toml --bin hub-cli -- fetch-models      # once, downloads ocrs models into ./models
    cargo run --manifest-path src-tauri/Cargo.toml --bin hub-cli -- ocr fixtures/screenshots/ingame-chat-1600x900.jpg [x y w h]
    cargo run --manifest-path src-tauri/Cargo.toml --bin hub-cli -- watch             # live: prints one JSON message per new chat line

`models/`, `maps/` and `settings.json` are all resolved relative to the
current working directory, so run the commands above from the repo root (or
adjust the paths if you run from elsewhere). The optional `[x y w h]` on
`ocr` overrides the default chat-box crop rectangle.

Settings live in `settings.json` next to the current working directory
(created on first save). Set `chatRect` to the pixel rectangle of the chat
box at your resolution and `selectedMap` to a folder name under `maps/`.
