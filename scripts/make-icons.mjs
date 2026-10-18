/**
 * Generates the Octopus Chat app icon set from one SVG source.
 *
 * Run with: npm run icons
 * Requires `sharp`, which is only a dev dependency and never ships.
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import sharp from "sharp";

const ICONS = join(process.cwd(), "src-tauri", "icons");
const SOURCE = join(ICONS, "octopus.svg");

/** Sizes the Tauri bundler expects, keyed by output name. */
const OUTPUTS = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 512],
  ["Square30x30Logo.png", 30],
  ["Square44x44Logo.png", 44],
  ["Square71x71Logo.png", 71],
  ["Square89x89Logo.png", 89],
  ["Square107x107Logo.png", 107],
  ["Square142x142Logo.png", 142],
  ["Square150x150Logo.png", 150],
  ["Square284x284Logo.png", 284],
  ["Square310x310Logo.png", 310],
  ["StoreLogo.png", 50],
];

const svg = readFileSync(SOURCE, "utf8");

mkdirSync(ICONS, { recursive: true });

for (const [name, size] of OUTPUTS) {
  const buf = await sharp(Buffer.from(svg), { density: 384 })
    .resize(size, size, { fit: "contain", background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();
  writeFileSync(join(ICONS, name), buf);
  console.log(`${name}  ${size}x${size}`);
}

// Windows .ico, assembled by hand so no extra tooling is needed.
const sizes = [16, 24, 32, 48, 64, 128, 256];
const frames = [];
for (const s of sizes) {
  frames.push(
    await sharp(Buffer.from(svg), { density: 384 })
      .resize(s, s, { fit: "contain", background: { r: 0, g: 0, b: 0, alpha: 0 } })
      .png()
      .toBuffer(),
  );
}
writeFileSync(join(ICONS, "icon.ico"), buildIco(frames, sizes));
console.log("icon.ico", sizes.join(","));

/** Minimal ICO container: header, directory entries, then PNG payloads. */
function buildIco(frames, sizes) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(frames.length, 4);

  const dir = Buffer.alloc(16 * frames.length);
  let offset = 6 + dir.length;
  frames.forEach((frame, i) => {
    const s = sizes[i];
    const at = i * 16;
    dir.writeUInt8(s >= 256 ? 0 : s, at); // width (0 means 256)
    dir.writeUInt8(s >= 256 ? 0 : s, at + 1); // height
    dir.writeUInt8(0, at + 2); // palette
    dir.writeUInt8(0, at + 3); // reserved
    dir.writeUInt16LE(1, at + 4); // colour planes
    dir.writeUInt16LE(32, at + 6); // bits per pixel
    dir.writeUInt32LE(frame.length, at + 8);
    dir.writeUInt32LE(offset, at + 12);
    offset += frame.length;
  });

  return Buffer.concat([header, dir, ...frames]);
}