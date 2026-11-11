# Lymra brand assets

The mark is a square cut into an **L** and a data stack of three blocks in its corner. That is the memory model in one picture. The top block, the most recently pushed value, is gold. Everything else is vermilion. Every cut has the same width, and only the four outer corners of the square are fully rounded, so the mark reads as one solid shape.

The animated version pushes the blocks one at a time, with gold always marking the top, then frees all of them at once, the way a scope exit frees a whole data stack with one store.

Every asset uses flat colors, with no gradients, and a transparent background.

## Files

| Asset | Use it for | Formats |
|---|---|---|
| `lymra-icon` | The main icon, 48 px and up. Works on light and dark backgrounds. | `svg/`, `png/lymra-icon/` at 48 to 1024 px |
| `lymra-icon-small` | 16 to 32 px: favicons, file icons, tabs | `svg/`, `png/lymra-icon-small/` at 16, 24, 32 px |
| `lymra-mark-white` | Photos and busy dark backgrounds where the colors would clash | `svg/`, `png/lymra-mark-white/` |
| `lymra-mark-mono` | One-ink printing, engraving, stamps. The SVG uses `currentColor`. | `svg/`, `png/lymra-mark-mono/` (black) |
| `lymra-wordmark` | Headers and docs on light backgrounds | `svg/`, `png/lymra-wordmark/` at 400, 800, 1600 px wide |
| `lymra-wordmark-white` | Headers and docs on dark backgrounds | `svg/`, `png/lymra-wordmark-white/` |
| `lymra-icon-animated` | Websites, splash screens, presentations | `animated/`: SVG, GIF and WebP at 256 and 512 px |
| `favicon.ico` | Browser favicon | 16, 32 and 48 px in one file |

The animated SVG stops on the full-stack frame when the viewer has reduced motion turned on. Use the WebP over the GIF where it is supported: GIF transparency is on or off, so its curved edges are slightly jagged against a background.

## Colors

| Name | Hex | Role |
|---|---|---|
| Vermilion | `#E8452C` | The letter and the lower stack blocks |
| Gold | `#F5A524` | The top stack block |
| Charcoal | `#1C1917` | Wordmark text on light backgrounds |

## Rules

- Below 48 px, use `lymra-icon-small`. The detailed icon turns to mush at 16 px.
- Don't put the icon on a tile or colored badge. It is designed to stand on the page by itself.
- Keep clear space around the mark equal to the width of the L's vertical stroke.
- Don't recolor, stretch, rotate, or add gradients, shadows, or other effects. Pick the variant made for the background.

## Rebuilding

Every file here is generated from `lib/marks.js`. Edit the geometry or colors there, never the output files.

Requirements: Node 18+, the `playwright` npm package with Chromium, and Python 3 with Pillow.

```bash
node branding/build.js
```
