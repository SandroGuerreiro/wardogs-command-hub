# Maps

One folder per map. `map.json` is the only committed file; `source.png`,
`tiles/` and `thumb.png` are generated or downloaded locally (gitignored).

- `id`: folder name, lowercase.
- `sourceSize`: pixel size of `source.png`. 16384 square for the three
  built-in maps (community hi-res captures, see SOURCE.md when added).
- `calibration`: `null` until you calibrate in the app. Two points, each
  with the in-game `x, y` and the pixel position on `source.png`.
- `places`: named locations in game coordinates, e.g. `"tower 5": {"x": 41.2, "y": 99.3}`.
- `aliases`: shorthand players type, mapped to a `places` key.

Add a map: create the folder, write `map.json`, drop `source.png`, run the
tiler (Plan 2). No code changes.
