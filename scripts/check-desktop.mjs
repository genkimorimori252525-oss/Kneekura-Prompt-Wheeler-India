import assert from "node:assert/strict";
import { readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const read = (path) => readFileSync(resolve(root, path));
const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
const caps = JSON.parse(read("src-tauri/capabilities/default.json"));
assert.ok(conf.build.frontendDist === "../desktop-dist");
assert.ok(conf.app.withGlobalTauri);
assert.ok(conf.app.windows[0].resizable);
assert.ok(conf.app.windows[0].alwaysOnTop);
assert.ok(caps.permissions.includes("core:window:allow-set-size"));
assert.ok(caps.permissions.includes("core:window:allow-set-always-on-top"));
assert.ok(caps.permissions.includes("core:window:allow-hide"));
const required = ["index.html","styles.css","app.js","desktop.js","desktop.css","data/prompts.json"];
for (const path of required) {
  const output = "desktop-dist/" + path;
  assert.ok(statSync(resolve(root, output)).size > 0, "Missing " + output);
}
const html = read("desktop-dist/index.html").toString("utf-8");
assert.match(html, /desktop\.js/);
assert.match(html, /quickCopy/);
const png = read("src-tauri/icons/icon.png");
assert.equal(png.subarray(0,8).toString("hex"), "89504e470d0a1a0a");
const ico = read("src-tauri/icons/icon.ico");
assert.equal(ico.readUInt16LE(2), 1);
assert.equal(ico.readUInt16LE(4), 1);
assert.equal(ico.readUInt32LE(14), png.length);
assert.ok(ico.subarray(22).equals(png));
const data = JSON.parse(read("desktop-dist/data/prompts.json"));
assert.equal(data.prompts.length, 45);
console.log("Desktop packaging preflight passed: static frontend, Tauri config and icon");
