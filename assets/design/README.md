# Design assets

Editable sources for shotpipe's icons. Edit in Figma/etc., export PNGs back
into this folder, then ask for the build to pick them up.

## Menu bar icon (template icon rules)

- **Single solid color + transparency** — macOS recolors template icons
  automatically for light/dark menu bars. Keep it pure black (#000000).
- Strong silhouette; readable at 18px. Avoid thin strokes (<8% of canvas).
- Two states: `menubar-active.*` (running) and `menubar-paused.*` (paused).
  Paused usually differs subtly (hollow vs filled element).

**Provide:** `menubar-active.png` and `menubar-paused.png` — square PNG with
alpha, single color, 256×256 or 512×512.

## App icon

- Rounded-square macOS style; the system masks square artwork, but baking
  ~23.7% corner radius yourself is also fine.
- Design at 1024×1024, keep the central motif legible when downscaled to 64px.

**Provide:** `app-icon.png` — 1024×1024 PNG (with transparency if you bake
the rounding yourself).
