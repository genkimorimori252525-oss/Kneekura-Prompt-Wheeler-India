import "./generate-icons.mjs";
import { copyFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const destination = resolve(root, "desktop-dist");
const allowedFiles = [
  "index.html",
  "styles.css",
  "app.js",
  "desktop.css",
  "desktop.js",
  "data/prompts.json"
];

for (const path of allowedFiles) {
  const target = resolve(destination, path);
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(resolve(root, path), target);
}
console.log("Prepared self-contained desktop frontend:", allowedFiles.join(", "));
