import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const catalog = JSON.parse(readFileSync(new URL("../data/prompts.json", import.meta.url), "utf8"));
assert.equal(catalog.schemaVersion, 1);
assert.ok(Array.isArray(catalog.categories) && catalog.categories.length >= 3);
assert.ok(Array.isArray(catalog.prompts) && catalog.prompts.length >= 6);
const categoryIds = new Set(catalog.categories.map(({id}) => id));
const ids = new Set();

for (const category of catalog.categories) {
  assert.equal(typeof category.label, "string");
  assert.ok(category.short && category.icon && /^#[0-9a-f]{6}$/i.test(category.color));
}
for (const item of catalog.prompts) {
  assert.ok(typeof item.id === "string" && /^[a-z0-9-]+$/.test(item.id), "invalid ID");
  assert.ok(!ids.has(item.id), "duplicate prompt ID: " + item.id);
  ids.add(item.id);
  assert.ok(categoryIds.has(item.category), "unknown category: " + item.id);
  for (const key of ["title", "summary", "prompt"]) {
    assert.ok(typeof item[key] === "string" && item[key].trim(), item.id + " missing " + key);
  }
  assert.ok(item.source && ["user-supplied", "adapted", "original"].includes(item.source.type), "invalid origin: " + item.id);
  assert.ok(typeof item.source.label === "string" && item.source.label.trim());
  if (item.source.type !== "original") assert.ok(/^https:\/\//.test(item.source.url), "missing source URL: " + item.id);
  if (item.promptEn !== undefined) assert.ok(typeof item.promptEn === "string" && item.promptEn.trim());
}
const originalVox = catalog.prompts.filter((p) => p.source.type === "user-supplied");
assert.equal(originalVox.length, 6, "provided screenshot set must have six entries");
assert.ok(originalVox.every((p) => p.promptEn), "original English must be preserved");

console.log("catalog valid:", catalog.prompts.length, "prompts,", catalog.categories.length, "categories");
