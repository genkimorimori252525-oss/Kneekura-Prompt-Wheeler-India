import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "desktop-dist");
const port = 1420;
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8"
};
const allowed = new Set([
  "/", "/index.html", "/styles.css", "/app.js",
  "/desktop.js", "/data/prompts.json"
]);

createServer(async (request, response) => {
  const url = new URL(request.url || "/", "http://127.0.0.1");
  const path = url.pathname;
  if (!allowed.has(path)) {
    response.writeHead(404);
    response.end("Not Found");
    return;
  }
  const normalized = path === "/" ? "/index.html" : path;
  const dot = normalized.lastIndexOf(".");
  const ext = normalized.slice(dot);
  try {
    const data = await readFile(resolve(root, "." + normalized));
    response.writeHead(200, {
      "Content-Type": types[ext] || "application/octet-stream",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff"
    });
    response.end(data);
  } catch (_) {
    response.writeHead(404);
    response.end("Missing File");
  }
}).listen(port, "127.0.0.1", () => {
  console.log("Desktop preview at http://127.0.0.1:" + port);
});
