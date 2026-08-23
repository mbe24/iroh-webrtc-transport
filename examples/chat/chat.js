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

const pc = new RTCPeerConnection({ iceServers: [{ urls: "stun:stun.l.google.com:19302" }] });
window._pc = pc; // keep alive
const ready = init();
let role = null, dc = null, chat = null;

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

// Resolve when ICE gathering completes, but never hang: host candidates arrive
// in milliseconds (enough for same-machine / LAN); STUN gets a short window for
// a srflx candidate, then we proceed with whatever is in the local description.
async function waitIceComplete(maxMs = 2000) {
  if (pc.iceGatheringState === "complete") return;
  await new Promise((res) => {
    const finish = () => { pc.removeEventListener("icegatheringstatechange", check); clearTimeout(timer); res(); };
    const check = () => { if (pc.iceGatheringState === "complete") finish(); };
    const timer = setTimeout(finish, maxMs);
    pc.addEventListener("icegatheringstatechange", check);
  });
}

pc.onconnectionstatechange = () => addMsg("sys", "peer connection: " + pc.connectionState);

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

ready.then(() => {
  setStatus(isManual ? "manual mode" : "ready");
  (isManual ? manualSignaling() : broadcastSignaling());
}).catch((e) => setStatus("wasm load failed: " + e));
