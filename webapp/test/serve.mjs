// Minimal static server for local dev (Web Serial needs a secure context —
// localhost counts; file:// does not). Zero deps.
//
//   node test/serve.mjs            (serves webapp/ on http://localhost:5173)
//   node test/serve.mjs 8080
//
// Or use any static server, e.g. `python -m http.server 5173`.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const port = Number(process.argv[2]) || 5173;
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
};

createServer(async (req, res) => {
  try {
    let rel = decodeURIComponent(req.url.split("?")[0]);
    if (rel === "/" || rel.endsWith("/")) rel += "live.html";
    const path = normalize(join(root, rel));
    if (!path.startsWith(root)) {
      res.statusCode = 403;
      return res.end("forbidden");
    }
    const body = await readFile(path);
    res.setHeader("content-type", TYPES[extname(path)] || "application/octet-stream");
    res.end(body);
  } catch {
    res.statusCode = 404;
    res.end("404");
  }
}).listen(port, () => console.log(`serving webapp/ on http://localhost:${port}/`));
