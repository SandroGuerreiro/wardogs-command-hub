# Wardogs Command Hub

Second-monitor tactical map for WARDOGS. Reads in-game team chat via screen
capture and OCR, drops pins for Mark Coordinates callouts and named places,
and keeps a scrolling feed so nothing gets lost.

Fan-made. Not affiliated with Bulkhead or Team17.

See `docs/superpowers/specs/` for the design.

## Running headless (Plan 1)

    cd src-tauri
    cargo run --bin hub-cli -- fetch-models      # once, downloads ocrs models into ./models
    cargo run --bin hub-cli -- ocr ../fixtures/screenshots/ingame-chat-1600x900.jpg
    cargo run --bin hub-cli -- watch             # live: prints one JSON message per new chat line

Settings live in `settings.json` next to the binary (created on first save).
Set `chatRect` to the pixel rectangle of the chat box at your resolution and
`selectedMap` to a folder name under `maps/`.
