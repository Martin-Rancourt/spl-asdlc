# Brand

The SPL-ASDLC logo has two forms, drawn from one geometry.

| Form | Reads | Use |
|---|---|---|
| Mark | `</>` | Favicon, GitHub App logo, tracker avatar, anywhere small |
| Wordmark | `<asdlc />` | README, documentation, site headers |

The `<` diverges: one change opens into several review angles. The `>` converges: the consolidator and the computed GO close it, and it is the only green stroke. The `/` is the passage between the two. Read together, it is a closing tag: the work is done.

## Colors

| Role | Light ground | Dark ground |
|---|---|---|
| Blue-grey: `<`, `/`, letters | `#54768D` | `#9DB1BD` |
| Green: the closing `>` | `#43986A` | `#4FAE7B` |
| Icon ground | `#FFFFFF` | `#17232C` |

The letters of the wordmark are IBM Plex Mono 600, converted to outlines: no asset needs a font.

## Files

Everything lives in `assets/brand/`.

| File | Content | Use |
|---|---|---|
| `mark.svg`, `mark-dark.svg` | `</>`, transparent ground | Inline in pages and documents |
| `icon.svg`, `icon-dark.svg` | `</>` on a square ground | Source of the app icons |
| `favicon.svg` | `</>`, follows the browser's light or dark theme | `<link rel="icon" type="image/svg+xml">` |
| `favicon.ico` | 16, 32 and 48 px | Fallback favicon |
| `wordmark.svg`, `wordmark-dark.svg` | `<asdlc />` | README and documentation headers |
| `png/icon-512.png`, `png/icon-dark-512.png` | 512 px square | GitHub App logo, tracker workspace or bot avatar |
| `png/apple-touch-icon-180.png` | 180 px square | `<link rel="apple-touch-icon">` |
| `png/favicon-16.png`, `-32.png`, `-48.png` | Transparent ground | Favicon sizes, also embedded in `favicon.ico` |

The mark stays inside the circle inscribed in its square, so circular avatar crops do not cut it.

## Regenerate

The geometry and the colors live in `scripts/brand/generate.mjs`. Change them there, never in the generated files, then run (macOS: the PNGs are rasterized by AppKit):

```bash
cd scripts/brand && npm install && node generate.mjs
```
