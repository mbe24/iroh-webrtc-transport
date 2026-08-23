// Dependency-free static file server for the chat demo. Signaling is serverless
// (BroadcastChannel between tabs, or manual link exchange across devices), so
// this only needs to serve files — no WebSocket relay.
//
//   node serve.mjs   →   http://127.0.0.1:8091   (open in two tabs to chat)
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const PORT = process.env.PORT || 8091;
const ROOT = new URL("./", import.meta.url);
const CT = {
  ".html": "text/html", ".js": "text/javascript", ".css": "text/css",
  ".wasm": "application/wasm", ".json": "application/json", ".ts": "text/plain",
};

http
  .createServer((req, res) => {
    let p = decodeURIComponent((req.url || "/").split("?")[0]);
    if (p === "/") p = "/index.html";
    fs.readFile(new URL("." + p, ROOT), (err, data) => {
      if (err) return void res.writeHead(404).end("not found: " + p);
      res.writeHead(200, { "content-type": CT[path.extname(p)] || "application/octet-stream" });
      res.end(data);
    });
  })
  .listen(PORT, "127.0.0.1", () =>
    console.log(`[iroh-webrtc chat] http://127.0.0.1:${PORT}  (static only; signaling + data are peer-to-peer)`)
  );
