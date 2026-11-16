// Tiny zero-dependency PNG/ICO generator. Bundles only a simple original icon.
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

const output = resolve(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
mkdirSync(output, { recursive: true });
const side = 128;
const pixels = Buffer.alloc(side * (1 + side * 4));
const paint = (x, y) => {
  const cx = x - 63.5, cy = y - 63.5;
  const r = Math.hypot(cx, cy);
  const cr = Math.max(Math.abs(cx) - 53, 0);
  const ct = Math.max(Math.abs(cy) - 53, 0);
  const insideTile = Math.hypot(cr, ct) < 8;
  if (!insideTile) return [0, 0, 0, 0];
  // Dark blue background, softly contrasted donut, mint rotation marker.
  if (cx > 26 && cx < 49 && cy < -28 && cy > -52) return [104, 224, 196, 255];
  if (r >= 27 && r <= 47) return [134, 156, 255, 255];
  if (r > 47 && r <= 49) return [76, 98, 187, 255];
  return [18, 30, 62, 255];
};
for (let y = 0; y < side; y++) {
  const start = y * (1 + side * 4);
  pixels[start] = 0; // PNG scanline filter: none
  for (let x = 0; x < side; x++) {
    const offset = start + 1 + x * 4;
    const color = paint(x, y);
    for (let c = 0; c < 4; c++) pixels[offset + c] = color[c];
  }
}
const crc = (bytes) => {
  let value = 0xffffffff;
  for (const byte of bytes) {
    value ^= byte;
    for (let k = 0; k < 8; k++) value = (value >>> 1) ^ ((value & 1) ? 0xedb88320 : 0);
  }
  return (value ^ 0xffffffff) >>> 0;
};
const chunk = (kind, bytes) => {
  const type = Buffer.from(kind, "ascii");
  const header = Buffer.alloc(4); header.writeUInt32BE(bytes.length);
  const checksum = Buffer.alloc(4); checksum.writeUInt32BE(crc(Buffer.concat([type, bytes])));
  return Buffer.concat([header, type, bytes, checksum]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(side, 0); ihdr.writeUInt32BE(side, 4);
ihdr[8] = 8; ihdr[9] = 6; // 8-bit RGBA
const png = Buffer.concat([
  Buffer.from([137,80,78,71,13,10,26,10]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(pixels, { level: 9 })),
  chunk("IEND", Buffer.alloc(0))
]);
writeFileSync(resolve(output, "icon.png"), png);

// A Vista+ ICO can store the same PNG without a second image compressor.
const icoHeader = Buffer.alloc(22);
icoHeader.writeUInt16LE(1, 2); // ICO type
icoHeader.writeUInt16LE(1, 4); // one embedded image
icoHeader[6] = side; icoHeader[7] = side;
icoHeader.writeUInt16LE(1, 10); icoHeader.writeUInt16LE(32, 12);
icoHeader.writeUInt32LE(png.length, 14);
icoHeader.writeUInt32LE(22, 18);
writeFileSync(resolve(output, "icon.ico"), Buffer.concat([icoHeader, png]));
console.log("Generated app icon (PNG and ICO) in src-tauri/icons");
