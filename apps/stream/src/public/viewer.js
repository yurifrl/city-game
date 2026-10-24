// city-game stream viewer — pixel-only client.
//
// Connects to /ws/{seatId}, draws the relayed JPEG screencast, and forwards
// pointer input back as JSON. Coordinates are translated from canvas CSS
// pixels into the seat's CSS viewport (from the hello message).

const canvas = document.getElementById('screen');
const overlay = document.getElementById('overlay');
const roleBadge = document.getElementById('role');
const stats = document.getElementById('stats');

if (!(canvas instanceof HTMLCanvasElement) || overlay === null || roleBadge === null || stats === null) {
  throw new Error('viewer DOM missing required elements');
}

const seatId = document.getElementById('seat-id')?.textContent ?? '';

let socket = null;
let hello = null;
let lastSeq = 0;
let frameCount = 0;
let fpsMark = performance.now();
let fps = 0;

function setOverlay(text) {
  if (text === null) {
    overlay.hidden = true;
  } else {
    overlay.textContent = text;
    overlay.hidden = false;
  }
}

function setRole(role) {
  roleBadge.textContent = role;
  roleBadge.classList.add(role);
  if (role === 'spectator') setOverlay('spectating — the first viewer of this seat controls input');
}

async function drawFrame(meta, jpegBytes) {
  const blob = new Blob([jpegBytes], { type: 'image/jpeg' });
  const bitmap = await createImageBitmap(blob);
  const ctx = canvas.getContext('2d');
  if (ctx === null) {
    bitmap.close();
    return;
  }
  canvas.width = meta.w;
  canvas.height = meta.h;
  ctx.drawImage(bitmap, 0, 0);
  bitmap.close();
  lastSeq = meta.seq;
  frameCount += 1;
  const now = performance.now();
  if (now - fpsMark >= 1000) {
    fps = Math.round((frameCount * 1000) / (now - fpsMark));
    frameCount = 0;
    fpsMark = now;
    stats.textContent = `${fps} fps · frame ${lastSeq}`;
  }
}

function connect() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  const ws = new WebSocket(`${proto}://${location.host}/ws/${encodeURIComponent(seatId)}`);
  ws.binaryType = 'arraybuffer';
  socket = ws;

  ws.onopen = () => setOverlay(null);

  ws.onmessage = (ev) => {
    if (typeof ev.data === 'string') {
      const msg = JSON.parse(ev.data);
      if (msg.type === 'hello') {
        hello = msg;
        setRole(msg.role);
      } else if (msg.type === 'error') {
        setOverlay(`seat error: ${msg.message}`);
        ws.close();
      }
      return;
    }
    const view = new DataView(ev.data);
    const metaLen = view.getUint32(0, false);
    const meta = JSON.parse(new TextDecoder().decode(new Uint8Array(ev.data, 4, metaLen)));
    const jpeg = new Uint8Array(ev.data, 4 + metaLen);
    drawFrame(meta, jpeg).catch((err) => console.error('frame decode failed', err));
  };

  ws.onclose = () => {
    setOverlay('disconnected — retrying…');
    socket = null;
    setTimeout(connect, 1000);
  };
}

// --- Input capture -----------------------------------------------------------

function sendInput(payload) {
  if (socket === null || socket.readyState !== WebSocket.OPEN || hello === null) return;
  socket.send(JSON.stringify({ type: 'input', ...payload }));
}

/** Canvas-local event position in seat CSS pixels. */
function seatPos(ev) {
  const rect = canvas.getBoundingClientRect();
  const vw = hello !== null ? hello.viewport.width : canvas.width;
  const vh = hello !== null ? hello.viewport.height : canvas.height;
  return {
    x: ((ev.clientX - rect.left) * vw) / rect.width,
    y: ((ev.clientY - rect.top) * vh) / rect.height,
  };
}

function buttonsOf(ev) {
  return ev.buttons & 0xffff;
}

canvas.addEventListener('pointermove', (ev) => {
  sendInput({ ...seatPos(ev), buttons: buttonsOf(ev), down: (ev.buttons & 1) !== 0 });
});

canvas.addEventListener('pointerdown', (ev) => {
  canvas.setPointerCapture(ev.pointerId);
  sendInput({ ...seatPos(ev), buttons: buttonsOf(ev), down: true });
});

canvas.addEventListener('pointerup', (ev) => {
  sendInput({ ...seatPos(ev), buttons: buttonsOf(ev), down: false });
});

canvas.addEventListener(
  'wheel',
  (ev) => {
    ev.preventDefault();
    sendInput({
      ...seatPos(ev),
      buttons: buttonsOf(ev),
      down: false,
      kind: 'wheel',
      deltaX: ev.deltaX,
      deltaY: ev.deltaY,
    });
  },
  { passive: false },
);

// Keepalive so proxies don't reap an idle viewer connection.
setInterval(() => {
  if (socket !== null && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: 'ping' }));
}, 30_000);

connect();
