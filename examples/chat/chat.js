// Browser-to-browser chat over iroh's QUIC on a WebRTC data channel.
//
// Signaling is 100% serverless (works on static hosting like GitHub Pages):
//   • same-browser tabs auto-pair over BroadcastChannel (default);
//   • across devices, exchange a compact offer/answer (link or QR, ?manual).
// The transport is iroh: JS opens the RTCDataChannel, then hands it to the wasm
// `start_chat`, which runs iroh's QUIC + a bi-stream over it. Data never touches
// a server.
import init, { start_chat } from "./pkg/iroh_webrtc_transport.js";

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const room = params.get("room") || "lobby";
const isManual = params.has("manual") || location.hash.startsWith("#offer=");

const setStatus = (t, ok = false) => { const s = $("status"); s.textContent = t; s.className = ok ? "ok" : ""; };
const addMsg = (who, text) => {
  const wrap = document.createElement("div");
  wrap.className = "msg " + who;
  if (who === "sys") wrap.innerHTML = `<span class="sys">${text}</span>`;
  else { const b = document.createElement("span"); b.className = "bubble"; b.textContent = text; wrap.appendChild(b); }
  $("log").appendChild(wrap);
  $("log").scrollTop = $("log").scrollHeight;
};

// The connection is created AFTER we've probed/ranked STUN servers (see
// resolveIceServers + bootstrap below), so its iceServers are the healthiest for
// this network right now.
let pc = null;
const ready = init();
let role = null, dc = null, chat = null;

// ---------- STUN probing: pool → probe on load → rank by latency → cache --------
// Each probe is ONE STUN Binding Request (a single tiny UDP round-trip — the same
// thing every WebRTC connection does; Google's public Trickle ICE tester works
// exactly this way, so it's standard, not abuse). We probe the pool in parallel on
// load, keep the reachable servers ranked fastest-first, and cache the ranking in
// sessionStorage with a short TTL: reloads and ?manual reuse it instantly, while a
// new tab / browser restart / >5-min-old cache re-probes (catching network changes).
const STUN_POOL = [
  "stun:stun.l.google.com:19302", // Google
  "stun:stun1.l.google.com:19302",
  "stun:stun2.l.google.com:19302",
  "stun:stun3.l.google.com:19302",
  "stun:stun4.l.google.com:19302",
  "stun:stun.cloudflare.com:3478", // Cloudflare
  "stun:global.stun.twilio.com:3478", // Twilio
  "stun:stun.relay.metered.ca:80", // Metered
  "stun:stun.nextcloud.com:443", // Nextcloud
  "stun:stun.sipgate.net:3478", // sipgate
  "stun:stun.sipgate.net:10000",
  "stun:stun.services.mozilla.com", // Mozilla (may be retired; the probe confirms)
];
const PROBE_TIMEOUT_MS = 2500;
const TOP_N = 4; // healthiest servers to actually use
const CACHE_KEY = "iroh-webrtc:stun-rank";
const CACHE_TTL_MS = 5 * 60 * 1000; // 5 minutes

// Probe one server: resolve {url, ok, ms} where ms is time-to-first-srflx (a
// public candidate proves the server answered), or unreachable after the timeout.
function probeStun(url) {
  return new Promise((resolve) => {
    let p;
    try { p = new RTCPeerConnection({ iceServers: [{ urls: url }] }); }
    catch { return resolve({ url, ok: false, ms: Infinity }); }
    const t0 = performance.now();
    let settled = false;
    let timer;
    const finish = (ok) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { p.onicecandidate = null; p.close(); } catch {}
      resolve({ url, ok, ms: ok ? performance.now() - t0 : Infinity });
    };
    timer = setTimeout(() => finish(false), PROBE_TIMEOUT_MS);
    p.onicecandidate = (e) => {
      const c = e.candidate;
      if (c && (c.type === "srflx" || /typ srflx/.test(c.candidate || ""))) finish(true);
    };
    try {
      p.createDataChannel("probe"); // a media section so gathering starts
      p.createOffer().then((o) => p.setLocalDescription(o)).catch(() => finish(false));
    } catch { finish(false); }
  });
}

function readStunCache() {
  try {
    const c = JSON.parse(sessionStorage.getItem(CACHE_KEY) || "null");
    if (c && c.ts && Array.isArray(c.ranked) && c.ranked.length && Date.now() - c.ts <= CACHE_TTL_MS) return c;
  } catch {}
  return null;
}

// Resolve iceServers for the real connection: a fresh cached ranking, or a live
// probe → rank reachable servers fastest-first → cache → take the top N. Falls
// back to the whole pool (likely LAN-only) if nothing answered.
async function resolveIceServers() {
  let cache = readStunCache();
  if (!cache) {
    setStatus("probing STUN servers…");
    const results = await Promise.all(STUN_POOL.map(probeStun));
    const ranked = results.filter((r) => r.ok).sort((a, b) => a.ms - b.ms).map((r) => r.url);
    cache = { ranked, ts: Date.now() };
    try { sessionStorage.setItem(CACHE_KEY, JSON.stringify(cache)); } catch {}
  }
  const top = cache.ranked.slice(0, TOP_N);
  if (!top.length) {
    addMsg("sys", "no STUN server responded to the probe — this link may be LAN-only");
    return STUN_POOL.map((urls) => ({ urls }));
  }
  addMsg("sys", `STUN ready: ${top.length} healthy of ${STUN_POOL.length} probed`);
  return top.map((urls) => ({ urls }));
}

function enableChat() {
  $("text").disabled = false; $("send").disabled = false; $("text").focus();
  setStatus("connected ✓ — say hi", true);
}
async function startTransport() {
  await ready;
  const initiator = role === "offerer";
  chat = await start_chat(dc, initiator ? 1 : 2, initiator ? 2 : 1, initiator, (text) => addMsg("peer", text));
  enableChat();
}
function wireChannel(channel) { dc = channel; dc.onopen = () => startTransport(); }
async function becomeOfferer() { role = "offerer"; wireChannel(pc.createDataChannel("iroh", { ordered: false, maxRetransmits: 0 })); }
function becomeAnswerer() { role = "answerer"; pc.ondatachannel = (e) => wireChannel(e.channel); }

$("send").onclick = () => {
  const t = $("text").value.trim();
  if (t && chat) { chat.send(t); addMsg("me", t); $("text").value = ""; }
};
$("text").addEventListener("keydown", (e) => { if (e.key === "Enter") $("send").click(); });
$("manualBtn").onclick = () => { location.href = location.pathname + "?manual"; };

// Resolve as soon as we have a routable path — a public (srflx/STUN) candidate,
// or gathering completion — so it's fast when STUN answers. A generous cap
// gives a slow STUN a real chance before we fall back to a LAN-only link, but
// we never hang.
async function waitIceComplete(capMs = 8000) {
  if (pc.iceGatheringState === "complete") return;
  await new Promise((res) => {
    const done = () => { cleanup(); res(); };
    const onState = () => { if (pc.iceGatheringState === "complete") done(); };
    const onCand = (e) => {
      const c = e.candidate;
      if (c && (c.type === "srflx" || /typ srflx/.test(c.candidate || ""))) done();
    };
    const timer = setTimeout(done, capMs);
    const cleanup = () => {
      clearTimeout(timer);
      pc.removeEventListener("icegatheringstatechange", onState);
      pc.removeEventListener("icecandidate", onCand);
    };
    pc.addEventListener("icegatheringstatechange", onState);
    pc.addEventListener("icecandidate", onCand);
  });
}

// ---------- Mode A: BroadcastChannel (same-browser tabs) ----------
// RTCSessionDescription / RTCIceCandidate aren't structured-cloneable, so send
// plain JSON-able objects over BroadcastChannel.
const sdpPlain = (d) => ({ type: d.type, sdp: d.sdp });

async function broadcastSignaling() {
  $("manualBtn").hidden = false;
  const myId = Math.random().toString(36).slice(2);
  const bc = new BroadcastChannel("iroh-webrtc:" + room);
  let peerId = null;
  pc.onicecandidate = (e) => { if (e.candidate) bc.postMessage({ t: "ice", c: e.candidate.toJSON() }); };
  bc.onmessage = async (ev) => {
    const m = ev.data;
    if (m.t === "hello") {
      if (peerId !== null) return;
      peerId = m.id;
      bc.postMessage({ t: "hello", id: myId });
      if (myId > peerId) {
        await becomeOfferer();
        await pc.setLocalDescription(await pc.createOffer());
        bc.postMessage({ t: "sdp", sdp: sdpPlain(pc.localDescription) });
      } else {
        becomeAnswerer();
      }
    } else if (m.t === "sdp") {
      await pc.setRemoteDescription(m.sdp);
      if (m.sdp.type === "offer") {
        await pc.setLocalDescription(await pc.createAnswer());
        bc.postMessage({ t: "sdp", sdp: sdpPlain(pc.localDescription) });
      }
    } else if (m.t === "ice") {
      try { await pc.addIceCandidate(m.c); } catch {}
    }
  };
  bc.postMessage({ t: "hello", id: myId });
  setStatus(`waiting for another tab in room “${room}” — open this page again`);
  addMsg("sys", "open this exact URL in a second tab to connect");
}

// ---------- SDP compaction (the ~crypto-floor link) ----------
// A data-channel SDP is 95% boilerplate; only ufrag/pwd/fingerprint/setup and
// the candidates vary. We ship just those (fingerprint as raw bytes) and rebuild
// a valid SDP on the other side — ~150 chars instead of multi-KB.
const b64u = (bytes) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const unb64u = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
const fpToB64 = (hex) => b64u(hex.split(":").map((h) => parseInt(h, 16)));
const b64ToFp = (b) => [...unb64u(b)].map((x) => x.toString(16).padStart(2, "0").toUpperCase()).join(":");

function packSdp(desc) {
  const s = desc.sdp;
  const g = (re) => (s.match(re) || [])[1] || "";
  const cands = [...s.matchAll(/a=candidate:\S+ \d+ udp \d+ (\S+) (\d+) typ (\S+)(?: raddr (\S+) rport (\d+))?/gi)]
    .map((m) => (m[3] === "host" ? `${m[1]},${m[2]}` : `${m[1]},${m[2]},${m[3][0]},${m[4]},${m[5]}`));
  const setup = g(/a=setup:(\S+)/);
  return [
    desc.type === "offer" ? "o" : "a",
    setup === "actpass" ? "A" : setup === "active" ? "a" : "p",
    g(/a=ice-ufrag:(\S+)/),
    g(/a=ice-pwd:(\S+)/),
    fpToB64(g(/a=fingerprint:sha-256 ([0-9A-Fa-f:]+)/i)),
    cands.join(";"),
  ].join("~");
}

function unpackSdp(packed) {
  const [t, su, ufrag, pwd, fpb, candStr] = packed.split("~");
  const setup = su === "A" ? "actpass" : su === "a" ? "active" : "passive";
  const expand = (c) => ({ h: "host", s: "srflx", r: "relay" }[c] || "host");
  const cands = candStr
    ? candStr.split(";").map((c, i) => {
        const p = c.split(",");
        return p.length === 2
          ? `a=candidate:${i} 1 udp ${2122252543 - i} ${p[0]} ${p[1]} typ host`
          : `a=candidate:${i} 1 udp ${1686052607 - i} ${p[0]} ${p[1]} typ ${expand(p[2])} raddr ${p[3]} rport ${p[4]}`;
      })
    : [];
  const lines = [
    "v=0", "o=- 0 0 IN IP4 0.0.0.0", "s=-", "t=0 0",
    "a=group:BUNDLE 0", "a=msid-semantic: WMS",
    "m=application 9 UDP/DTLS/SCTP webrtc-datachannel", "c=IN IP4 0.0.0.0",
    `a=ice-ufrag:${ufrag}`, `a=ice-pwd:${pwd}`,
    `a=fingerprint:sha-256 ${b64ToFp(fpb)}`, `a=setup:${setup}`, "a=mid:0",
    "a=sctp-port:5000", "a=max-message-size:262144", ...cands,
  ];
  return { type: t === "o" ? "offer" : "answer", sdp: lines.join("\r\n") + "\r\n" };
}

function linkFor(hash) { return location.origin + location.pathname + hash; }
function renderQR(text) {
  const el = $("qr");
  el.innerHTML = "";
  try {
    const qr = window.qrcode(0, "L"); // auto version, low EC (max capacity)
    qr.addData(text);
    qr.make();
    el.innerHTML = `<img alt="QR" src="${qr.createDataURL(5, 2)}" />`;
  } catch (e) {
    el.textContent = "(QR unavailable: " + e.message + ")";
  }
}

// ---------- Mode B: manual link / QR (cross-device, no server) ----------
// A "public" (srflx) candidate is what lets a peer on another network reach us.
// If STUN didn't answer within the cap, we only have host candidates → the link
// works on the same machine / LAN but not across the internet. Say so.
const hasPublicCandidate = () => /typ srflx/.test(pc.localDescription.sdp || "");
const lanOnlyWarning = () =>
  hasPublicCandidate()
    ? ""
    : `<p style="color:#b26a00">⚠ LAN-only: no public (STUN) candidate was gathered in time, so this link will only connect on the same machine or local network — not across the internet. Reload to retry.</p>`;

async function manualSignaling() {
  $("manual").hidden = false;
  pc.onicecandidate = () => {}; // non-trickle: candidates ride in the (compacted) SDP
  const offerParam = new URLSearchParams(location.hash.slice(1)).get("offer");
  if (offerParam) {
    becomeAnswerer();
    await pc.setRemoteDescription(unpackSdp(decodeURIComponent(offerParam)));
    await pc.setLocalDescription(await pc.createAnswer());
    setStatus("gathering network path…");
    await waitIceComplete();
    const link = linkFor("#answer=" + encodeURIComponent(packSdp(pc.localDescription)));
    setStatus(hasPublicCandidate() ? "answer ready — send it back to the offerer" : "answer ready (LAN-only)");
    $("manual-body").innerHTML =
      `${lanOnlyWarning()}<p>Send this <b>answer</b> back (scan or copy):</p><div class="link">${link}</div>`;
    renderQR(link);
  } else {
    await becomeOfferer();
    await pc.setLocalDescription(await pc.createOffer());
    setStatus("gathering network path…");
    await waitIceComplete();
    const link = linkFor("#offer=" + encodeURIComponent(packSdp(pc.localDescription)));
    setStatus(hasPublicCandidate() ? "offer ready — share it, then paste the answer" : "offer ready (LAN-only)");
    $("manual-body").innerHTML =
      `${lanOnlyWarning()}<p>Share this <b>offer</b> with your peer (scan or copy):</p><div class="link">${link}</div>
       <p>Paste the <b>answer</b> they send back:</p>
       <textarea id="ans" placeholder="paste the #answer=… link"></textarea>
       <button id="applyAns">connect</button>`;
    renderQR(link);
    $("applyAns").onclick = async () => {
      const v = $("ans").value.trim();
      const a = new URLSearchParams(v.slice(v.indexOf("#") + 1)).get("answer");
      if (a) { await pc.setRemoteDescription(unpackSdp(decodeURIComponent(a))); setStatus("answer applied — connecting…"); }
    };
  }
}

// Bootstrap: probe/rank STUN → create the connection with the healthiest servers
// → wait for wasm → start signaling.
async function bootstrap() {
  const iceServers = await resolveIceServers();
  pc = new RTCPeerConnection({ iceServers });
  window._pc = pc; // keep alive
  pc.onconnectionstatechange = () => addMsg("sys", "peer connection: " + pc.connectionState);
  await ready;
  setStatus(isManual ? "manual mode" : "ready");
  (isManual ? manualSignaling() : broadcastSignaling());
}
bootstrap().catch((e) => setStatus("startup failed: " + e));
