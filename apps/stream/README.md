# apps/stream — server-rendered video bridge

Productionized form of the M2 video PoC: headless Chrome renders the real game
client on the host, the seat relays its screen as JPEG frames over a WebSocket,
and viewer input is injected back through CDP. Viewers need no WebGL — a
pixel-only `<canvas>` plus mouse events.

## Wire surface

| Route                | Meaning                                                        |
| -------------------- | -------------------------------------------------------------- |
| `GET /api/status.json` | Pool snapshot: seats, states (`booting/idle/ready/busy/error`), viewers, pids |
| `POST /api/seat`     | Boot a fresh seat → `{"id":"seat-…"}`                          |
| `DELETE /api/seat?id=…` | Destroy a seat (204; 404 unknown)                           |
| `GET /api/seed`      | Proxied upstream seed endpoint (game server `/api/seed`)       |
| `GET /view`          | Redirect to the least-loaded usable seat's viewer              |
| `GET /view/{seatId}` | Viewer page (seat id + viewport baked into the HTML)           |
| `WS /ws/{seatId}`    | Viewer socket: JSON hello → binary frames; input JSON upstream |

Viewer protocol (first message service → viewer):
`{"type":"hello","role":"controller"|"spectator","seatId","viewport":{width,height,dpr}}`.
Every subsequent service → viewer message is one binary frame:
`u32 BE meta length | JSON meta | JPEG bytes`, where meta is
`{seq, format:"jpeg", w, h, cssWidth, cssHeight, dpr, ts}` (w/h are the
JPEG's intrinsic device pixels; css* the seat CSS viewport).

Viewer → service messages: `{"type":"input", x, y, buttons, down, deltaX?, deltaY?}`
(controller only — the first attached viewer; everyone else is a spectator and
their input is dropped) and the text keepalive `"ping"` → `"pong"`.

On seat failure the socket receives `{"type":"error","message":…}` and closes.

## Run order

1. Game server (crates/server) listening on `CITY_GAME_URL`
   (default `http://127.0.0.1:8080`).
2. This service: `bun run src/index.ts` — HTTP+WS on `CITY_STREAM_PORT`
   (default 8081); it boots `CITY_SEATS` warm seats (default 2).
3. Open `http://127.0.0.1:8081/view` (redirects to a live seat's viewer).

Without chrome installed, run everything with `CITY_STREAM_FAKE=1`: an
in-process fake CDP server replaces chrome (tiny JPEG frames, recorded input
dispatches) — the same mode the test suite uses, so the whole surface is
exercisable chrome-free.

## Environment

| Var                          | Default      | Meaning                          |
| ---------------------------- | ------------ | -------------------------------- |
| `CITY_STREAM_PORT`           | `8081`       | listen port                      |
| `CITY_GAME_URL`              | `:8080`      | upstream game server             |
| `CITY_SEATS`                 | `2`          | warm pool size (janitor-maintained) |
| `CITY_STREAM_WIDTH/HEIGHT`   | `1280/800`   | seat CSS viewport                |
| `CITY_STREAM_DPR`            | `1`          | device scale factor              |
| `CITY_STREAM_QUALITY`        | `60`         | screencast JPEG quality          |
| `CITY_STREAM_CHROME`         | auto-detect  | chrome binary path               |
| `CITY_STREAM_FAKE`           | off          | `1/true/yes/on` → fake CDP       |
| `CITY_STREAM_BOOT_TIMEOUT_MS`| `30000`      | spawn → first-frame budget       |
| `CITY_STREAM_JANITOR_MS`     | `5000`       | reap/top-up interval             |

## Tests

`bun test apps/stream`

- `test/happy-path.test.ts` — full fake-mode service: HTTP surface, two-viewer
  claim + input routing + frame relay, keepalive, seat lifecycle, janitor
  auto-destroy, boot-failure self-clean, chrome-log filter, codec round-trip.
- `test/unit.test.ts` — env parsing, helpers, CDP client, JPEG sniffer.
- `test/real-chrome.test.ts` — real headless chrome integration; skipped
  cleanly when no chrome binary is found.

Source layout: `src/seat.ts` (chrome/CDP lifecycle + input translation),
`src/pool.ts` (warm pool, viewer registry, janitor), `src/service.ts`
(HTTP+WS front), `src/cdp.ts` (flat CDP client), `src/fake-cdp.ts`
(chrome-less endpoint), `src/public/viewer.*` (pixel viewer).
