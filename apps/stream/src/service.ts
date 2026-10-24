// HTTP + WebSocket front for the seat pool.
//
//   GET  /api/status.json     pool snapshot
//   POST /api/seat            boot a fresh seat → {id}
//   DEL  /api/seat?id=…       destroy a seat
//   GET  /api/seed            proxied upstream seed endpoint (parity/tests)
//   GET  /view                redirect to the best seat's viewer
//   GET  /view/{seatId}       viewer page (seat id + viewport baked in)
//   WS   /ws/{seatId}         attach as viewer: hello → binary frames → input

import type { ServerWebSocket, Server } from 'bun';
import { SeatPool, PoolError } from './pool';
import type { Frame } from './seat';
import type { FrameMeta, ViewerError, ViewerHello, ViewerInput } from './types';

interface WsData {
  seatId: string;
  viewerId: string;
  role: 'controller' | 'spectator';
  unsubscribe: () => void;
}

/** Validate a viewer JSON message into a clean ViewerInput (null = ignore). */
function asViewerInput(v: unknown): ViewerInput | null {
  if (typeof v !== 'object' || v === null || !('type' in v) || v.type !== 'input') return null;
  if (!('x' in v) || !('y' in v) || !('buttons' in v) || !('down' in v)) return null;
  const { x, y, buttons, down } = v;
  if (typeof x !== 'number' || typeof y !== 'number' || typeof buttons !== 'number' || typeof down !== 'boolean') return null;
  if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(buttons)) return null;
  const input: ViewerInput = { type: 'input', x, y, buttons, down };
  if ('kind' in v) {
    if (v.kind !== 'wheel') return null;
    input.kind = 'wheel';
    if ('deltaX' in v && typeof v.deltaX === 'number') input.deltaX = v.deltaX;
    if ('deltaY' in v && typeof v.deltaY === 'number') input.deltaY = v.deltaY;
  }
  return input;
}

/** u32be length + JSON meta + JPEG — one binary frame to a viewer. */
function framePacket(frame: Frame, cssWidth: number, cssHeight: number, dpr: number): Uint8Array {
  const meta: FrameMeta = {
    format: 'jpeg',
    seq: frame.seq,
    w: frame.w,
    h: frame.h,
    cssWidth,
    cssHeight,
    dpr,
    ts: frame.ts,
  };
  const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
  const packet = new Uint8Array(4 + metaBytes.length + frame.jpeg.length);
  const view = new DataView(packet.buffer);
  view.setUint32(0, metaBytes.length, false);
  packet.set(metaBytes, 4);
  packet.set(frame.jpeg, 4 + metaBytes.length);
  return packet;
}

function jsonRes(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

function errorRes(err: unknown): Response {
  if (err instanceof PoolError) return jsonRes({ error: err.message }, err.status);
  const message = err instanceof Error ? err.message : String(err);
  return jsonRes({ error: message }, 500);
}

/**
 * Build the Bun.serve handler set against a pool. `viewerTemplate` is the raw
 * viewer.html (placeholder tokens replaced per request).
 */
export function makeService(pool: SeatPool, viewerTemplate: string): {
  fetch: (req: Request, server: Server<WsData>) => Promise<Response | undefined>;
  websocket: {
    open: (ws: ServerWebSocket<WsData>) => void;
    message: (ws: ServerWebSocket<WsData>, raw: string | Buffer) => void;
    close: (ws: ServerWebSocket<WsData>) => void;
  };
} {
  return {
    async fetch(req, server): Promise<Response | undefined> {
      try {
        const url = new URL(req.url);
        const path = url.pathname;

        // --- WebSocket viewer attach -----------------------------------
        if (path.startsWith('/ws/')) {
          const seatId = decodeURIComponent(path.slice('/ws/'.length));
          const attach = pool.attachViewer(seatId);
          const data: WsData = {
            seatId: attach.seatId,
            viewerId: attach.viewerId,
            role: attach.role,
            unsubscribe: () => {},
          };
          if (!server.upgrade(req, { data })) return jsonRes({ error: 'websocket upgrade failed' }, 400);
          return undefined; // upgrade took over the socket
        }

        // --- REST --------------------------------------------------------
        if (path === '/api/status.json') return jsonRes(pool.status());

        if (path === '/api/seed') {
          const upstream = await fetch(new URL('/api/seed', `${pool.gameUrl}/`));
          return new Response(await upstream.text(), {
            status: upstream.status,
            headers: { 'content-type': upstream.headers.get('content-type') ?? 'application/json' },
          });
        }

        if (path === '/api/seat' && req.method === 'POST') {
          const seat = await pool.createSeat();
          return jsonRes({ id: seat.id });
        }

        if (path === '/api/seat' && req.method === 'DELETE') {
          const id = url.searchParams.get('id');
          if (id === null) return jsonRes({ error: 'missing ?id=' }, 400);
          await pool.destroySeat(id);
          return new Response(null, { status: 204 });
        }

        // --- Static viewer ------------------------------------------------
        if (path.startsWith('/static/')) {
          const name = path.slice('/static/'.length);
          // Whitelist: no traversal, only the two viewer assets.
          if (name !== 'viewer.css' && name !== 'viewer.js') return jsonRes({ error: 'not found' }, 404);
          const file = Bun.file(new URL(`./public/${name}`, import.meta.url));
          const type = name.endsWith('.css') ? 'text/css; charset=utf-8' : 'text/javascript; charset=utf-8';
          return new Response(file, { headers: { 'content-type': type } });
        }

        if (path === '/' || path === '/view' || path === '/view/') {
          const best = pool.bestSeatId();
          if (best === undefined) return jsonRes({ error: 'no seats; POST /api/seat to boot one' }, 503);
          return new Response(null, { status: 302, headers: { location: `/view/${best}` } });
        }

        if (path.startsWith('/view/')) {
          const seatId = decodeURIComponent(path.slice('/view/'.length));
          if (pool.get(seatId) === undefined) return jsonRes({ error: `unknown seat: ${seatId}` }, 404);
          const html = viewerTemplate
            .replaceAll('__SEAT_ID__', seatId)
            .replaceAll('__WIDTH__', String(pool.env.width))
            .replaceAll('__HEIGHT__', String(pool.env.height))
            .replaceAll('__DPR__', String(pool.env.dpr));
          return new Response(html, { headers: { 'content-type': 'text/html; charset=utf-8' } });
        }

        return jsonRes({ error: 'not found' }, 404);
      } catch (err) {
        return errorRes(err);
      }
    },

    websocket: {
      open(ws): void {
        const seat = pool.get(ws.data.seatId);
        if (seat === undefined) {
          const err: ViewerError = { type: 'error', message: `seat vanished: ${ws.data.seatId}` };
          ws.send(JSON.stringify(err));
          ws.close();
          return;
        }
        const send = (frame: Frame): void => {
          try {
            ws.send(framePacket(frame, pool.env.width, pool.env.height, pool.env.dpr));
          } catch {
            /* socket closed mid-send */
          }
        };
        ws.data.unsubscribe = seat.onFrame(send);
        const hello: ViewerHello = {
          type: 'hello',
          role: ws.data.role,
          seatId: ws.data.seatId,
          viewport: pool.viewport(),
        };
        ws.send(JSON.stringify(hello));
        // Catch the freshly attached viewer up so the canvas isn't blank.
        const last = seat.lastFrame;
        if (last !== null) send(last);
      },

      message(ws, raw): void {
        const text = typeof raw === 'string' ? raw : Buffer.from(raw).toString('utf8');
        let parsed: unknown;
        try {
          parsed = JSON.parse(text);
        } catch {
          return; // stray non-JSON frame: ignore, keep the socket
        }
        if (typeof parsed === 'object' && parsed !== null && 'type' in parsed && parsed.type === 'ping') {
          return; // viewer keepalive
        }
        const input = asViewerInput(parsed);
        if (input === null) return;
        pool.dispatchInput(ws.data.seatId, ws.data.viewerId, input).catch(() => {
          /* spectator input, unknown seat, or chrome hiccup: drop it */
        });
      },

      close(ws): void {
        ws.data.unsubscribe();
        pool.detachViewer(ws.data.seatId, ws.data.viewerId);
      },
    },
  };
}
