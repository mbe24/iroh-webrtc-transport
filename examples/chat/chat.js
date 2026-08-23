// Browser↔browser chat over iroh's QUIC on a WebRTC data channel.
//
// Signaling is 100% serverless (works on static hosting like GitHub Pages):
//   • same-browser tabs auto-pair over BroadcastChannel (default);
//   • across devices, exchange an offer/answer link (?manual).
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

// STUN helps across-NAT; harmless on localhost/LAN (host candidates win).
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

$("send").onclick = () => {
  const t = $("text").value.trim();
  if (t && chat) { chat.send(t); addMsg("me", t); $("text").value = ""; }
};
$("text").addEventListener("keydown", (e) => { if (e.key === "Enter") $("send").click(); });

// role: "offerer" creates the data channel; "answerer" receives it.
async function becomeOfferer() { role = "offerer"; wireChannel(pc.createDataChannel("iroh", { ordered: false, maxRetransmits: 0 })); }
function becomeAnswerer() { role = "answerer"; pc.ondatachannel = (e) => wireChannel(e.channel); }

async function waitIceComplete() {
  if (pc.iceGatheringState === "complete") return;
  await new Promise((res) => pc.addEventListener("icegatheringstatechange", () => pc.iceGatheringState === "complete" && res()));
}

// ---------- Mode A: BroadcastChannel (same-browser tabs) ----------
// RTCSessionDescription / RTCIceCandidate are platform objects that are NOT
// structured-cloneable, so BroadcastChannel.postMessage would throw on them.
// Send plain JSON-able objects instead.
const sdpPlain = (d) => ({ type: d.type, sdp: d.sdp });

async function broadcastSignaling() {
  const myId = Math.random().toString(36).slice(2);
  const bc = new BroadcastChannel("iroh-webrtc:" + room);
  let peerId = null;
  pc.onicecandidate = (e) => { if (e.candidate) bc.postMessage({ t: "ice", c: e.candidate.toJSON() }); };
  bc.onmessage = async (ev) => {
    const m = ev.data;
    if (m.t === "hello") {
      if (peerId !== null) return;          // already paired
      peerId = m.id;
      bc.postMessage({ t: "hello", id: myId }); // let the peer learn us too
      if (myId > peerId) {                  // deterministic role election
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

// ---------- Mode B: manual link exchange (cross-device, no server) ----------
const enc = (o) => btoa(JSON.stringify(o));
const dec = (s) => JSON.parse(atob(s));
function showManual(html) { $("manual").open = true; $("manual-body").innerHTML = html; }
function linkFor(hash) { return location.origin + location.pathname + hash; }

async function manualSignaling() {
  pc.onicecandidate = () => {}; // non-trickle: candidates ride in the SDP after gathering
  const offerParam = new URLSearchParams(location.hash.slice(1)).get("offer");
  if (offerParam) {
    becomeAnswerer();
    await pc.setRemoteDescription(dec(offerParam));
    await pc.setLocalDescription(await pc.createAnswer());
    await waitIceComplete();
    const link = linkFor("#answer=" + enc(pc.localDescription));
    setStatus("answer ready — send the link back to the offerer");
    showManual(`<p>Send this <b>answer link</b> back to whoever gave you the offer:</p>
      <textarea readonly onclick="this.select()">${link}</textarea>`);
  } else {
    await becomeOfferer();
    await pc.setLocalDescription(await pc.createOffer());
    await waitIceComplete();
    const link = linkFor("#offer=" + enc(pc.localDescription));
    setStatus("offer ready — share the link, then paste the answer");
    showManual(`<p>Share this <b>offer link</b> with your peer:</p>
      <textarea readonly onclick="this.select()">${link}</textarea>
      <p>Paste the <b>answer link</b> they send back:</p>
      <textarea id="ans" placeholder="paste #answer=… link here"></textarea>
      <button id="applyAns">connect</button>`);
    $("applyAns").onclick = async () => {
      const v = $("ans").value.trim();
      const a = new URLSearchParams(v.slice(v.indexOf("#") + 1)).get("answer");
      if (a) { await pc.setRemoteDescription(dec(a)); setStatus("answer applied — connecting…"); }
    };
  }
}

pc.onconnectionstatechange = () => addMsg("sys", "peer connection: " + pc.connectionState);

ready.then(() => {
  setStatus(isManual ? "manual mode" : "ready");
  (isManual ? manualSignaling() : broadcastSignaling());
}).catch((e) => setStatus("wasm load failed: " + e));
