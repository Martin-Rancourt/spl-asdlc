// Generates every SPL-ASDLC logo asset from one geometry (docs/brand.md): the mark </> and the
// wordmark <asdlc />. The « < » diverges (the review angles open up), the « > » converges (the
// consolidator, the computed GO) and is the only green stroke. Type: IBM Plex Mono 600, outlined,
// so no asset needs a font.
//
//   cd scripts/brand && npm install && node generate.mjs
//
// Writes assets/brand/*.svg, assets/brand/png/*.png and assets/brand/favicon.ico. PNGs are
// rasterized by AppKit (rasterize.swift), so this runs on macOS only.
import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import opentype from "opentype.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "../..");
const A = "assets/brand";
const out = (rel, data) => { const p = join(ROOT, rel); mkdirSync(dirname(p), { recursive: true }); writeFileSync(p, data); return p; };

// ---------- colors (light ground / dark ground) ----------
// `main` = blue-grey (chevron, slash, letters), `go` = green (the closing « > » only).
const COLORS = {
  light: { main: "#54768D", go: "#43986A", ground: "#FFFFFF" },
  dark: { main: "#9DB1BD", go: "#4FAE7B", ground: "#17232C" },
};

const svg = (vb, body) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vb}">${body}</svg>\n`;
const stroke = (d, color, w) => `<path d="${d}" fill="none" stroke="${color}" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round"/>`;

// ---------- the mark </>, on a 32 × 32 grid ----------
const MARK = {
  open: "M9.5 9.5 L3.5 16 L9.5 22.5",
  slash: "M18 8 L14 24",
  close: "M22.5 9.5 L28.5 16 L22.5 22.5",
  width: 2.8,
};
const markBody = (c) => stroke(`${MARK.open} ${MARK.slash}`, c.main, MARK.width) + stroke(MARK.close, c.go, MARK.width);
const icon = (c) => svg("0 0 32 32", `<rect width="32" height="32" fill="${c.ground}"/>` + markBody(c));

// The favicon follows the browser's theme; transparent ground.
const favicon = svg("0 0 32 32",
  `<style>.m{stroke:${COLORS.light.main}}.g{stroke:${COLORS.light.go}}`
  + `@media (prefers-color-scheme:dark){.m{stroke:${COLORS.dark.main}}.g{stroke:${COLORS.dark.go}}}</style>`
  + `<path class="m" d="${MARK.open} ${MARK.slash}" fill="none" stroke-width="${MARK.width}" stroke-linecap="round" stroke-linejoin="round"/>`
  + `<path class="g" d="${MARK.close}" fill="none" stroke-width="${MARK.width}" stroke-linecap="round" stroke-linejoin="round"/>`);

// ---------- the wordmark <asdlc />, letters in IBM Plex Mono 600, outlined ----------
const font = opentype.parse(readFileSync(join(HERE, "node_modules/@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-600-normal.woff")).buffer);
const WORD = {
  open: "M56 32 L36 50 L56 68",
  slash: "M262 24 L246 76",
  close: "M280 32 L300 50 L280 68",
  width: 6.5,
  text: { x: 66, baseline: 64, size: 48 },
  viewBox: "28 18 280 64",
};
function wordmark(c) {
  const t = WORD.text;
  const letters = font.getPath("asdlc", t.x, t.baseline, t.size).toPathData(2);
  return svg(WORD.viewBox,
    stroke(`${WORD.open} ${WORD.slash}`, c.main, WORD.width)
    + `<path d="${letters}" fill="${c.main}"/>`
    + stroke(WORD.close, c.go, WORD.width));
}

// ---------- write the SVG masters ----------
const files = {
  "mark.svg": svg("0 0 32 32", markBody(COLORS.light)),
  "mark-dark.svg": svg("0 0 32 32", markBody(COLORS.dark)),
  "icon.svg": icon(COLORS.light),
  "icon-dark.svg": icon(COLORS.dark),
  "favicon.svg": favicon,
  "wordmark.svg": wordmark(COLORS.light),
  "wordmark-dark.svg": wordmark(COLORS.dark),
};
for (const [name, text] of Object.entries(files)) out(`${A}/${name}`, text);

// ---------- PNGs (AppKit) ----------
const jobs = [
  ["icon.svg", "icon-512.png", 512],
  ["icon-dark.svg", "icon-dark-512.png", 512],
  ["icon.svg", "apple-touch-icon-180.png", 180],
  ["mark.svg", "favicon-16.png", 16],
  ["mark.svg", "favicon-32.png", 32],
  ["mark.svg", "favicon-48.png", 48],
];
mkdirSync(join(ROOT, A, "png"), { recursive: true });
execFileSync("swift", [join(HERE, "rasterize.swift"), ...jobs.flatMap(([i, o, s]) => [join(ROOT, A, i), join(ROOT, A, "png", o), String(s)])], { stdio: "inherit" });

// ---------- favicon.ico: the 16, 32 and 48 px PNGs, embedded as is ----------
const icoSizes = [16, 32, 48];
const pngs = icoSizes.map((s) => readFileSync(join(ROOT, A, "png", `favicon-${s}.png`)));
const header = Buffer.alloc(6 + 16 * pngs.length);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(pngs.length, 4);
let offset = header.length;
pngs.forEach((png, i) => {
  const e = 6 + 16 * i;
  header.writeUInt8(icoSizes[i], e);
  header.writeUInt8(icoSizes[i], e + 1);
  header.writeUInt16LE(1, e + 4);
  header.writeUInt16LE(32, e + 6);
  header.writeUInt32LE(png.length, e + 8);
  header.writeUInt32LE(offset, e + 12);
  offset += png.length;
});
out(`${A}/favicon.ico`, Buffer.concat([header, ...pngs]));

console.log(`brand: ${Object.keys(files).length} SVG masters, ${jobs.length} PNGs, favicon.ico written to ${A}`);
