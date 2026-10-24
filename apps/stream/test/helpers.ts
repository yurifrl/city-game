// Shared test helpers: deterministic env, upstream stub, service runner,
// frame-packet parsing, and a WebSocket viewer client.

import { loadStreamEnv, type StreamEnv } from '../src/env';
import { makeService } from '../src/service';
import { SeatPool } from '../src/pool';
import { waitFor } from '../src/ids';
import type { FrameMeta, ViewerHello } from '../src/types';

/** Stream env for fake-CDP tests: no chrome, tiny viewport, fast janitor. */
export function testEnv(overrides: Record<string, string> = {}, gameUrl = 'http://127.0.0.1:1'): StreamEnv {
  return loadStreamEnv({
    CITY_STREAM_FAKE: '1',
    CITY_SEATS: '1',
    CITY_STREAM_WIDTH: '320',
    CITY_STREAM_HEIGHT: '240',
    CITY_STREAM_DPR: '1',
    CITY_STREAM_QUALITY: '60',
    CITY_STREAM_BOOT_TIMEOUT_MS: '10000',
    CITY_STREAM_JANITOR_MS: '50',
    CITY_GAME_URL: gameUrl,
    ...overrides,
  });
}

/** Stand-in for the Rust game server (only /api/seed matters to the proxy). */
export async function startUpstream(seed = 'test-seed-00000000'): Promise<{ url: string; stop(): void }> {
  const server = Bun.serve({
    port: 0,
    fetch: () => Response.json({ seed }),
  });
  return { url: `http://127.0.0.1:${server.port}`, stop: () => server.stop(true) };
}

/** HTTP+WS server over an ephemeral port serving the given pool. */
export async function startStream(pool: SeatPool, viewerHtml: string): Promise<{ url: string; stop(): void }> {
  const svc = makeService(pool, viewerHtml);
  const server = Bun.serve({ port: 0, fetch: svc.fetch, websocket: svc.websocket });
  return { url: `http://127.0.0.1:${server.port}`, stop: () => server.stop(true) };
}

/** Viewer page template, read once. */
export async function viewerTemplate(): Promise<string> {
  return Bun.file(new URL('../src/public/viewer.html', import.meta.url)).text();
}

/** Per-field narrowing of the service hello (no casts on external input). */
function asHello(v: object): ViewerHello {
  if (!('role' in v) || (v.role !== 'controller' && v.role !== 'spectator')) throw new Error(`bad hello role: ${JSON.stringify(v)}`);
  if (!('seatId' in v) || typeof v.seatId !== 'string') throw new Error(`bad hello seatId: ${JSON.stringify(v)}`);
  if (!('viewport' in v) || typeof v.viewport !== 'object' || v.viewport === null) throw new Error(`bad hello viewport: ${JSON.stringify(v)}`);
  const vp = v.viewport;
  const width = fieldNum(vp, 'width');
  const height = fieldNum(vp, 'height');
  const dpr = fieldNum(vp, 'dpr');
  if ([width, height, dpr].some((n) => Number.isNaN(n))) throw new Error(`bad hello viewport: ${JSON.stringify(v)}`);
  return { type: 'hello', role: v.role, seatId: v.seatId, viewport: { width, height, dpr } };
}

function fieldNum(obj: object, key: string): number {
  for (const [k, value] of Object.entries(obj)) {
    if (k === key && typeof value === 'number') return value;
  }
  return Number.NaN;
}

export interface FramePacket {
  meta: FrameMeta;
  jpeg: Uint8Array;
}

/** Decode the 4-byte BE length + JSON meta + JPEG binary wire frame. */
export function parseFramePacket(bytes: Uint8Array): FramePacket {
  if (bytes.length < 4) throw new Error(`frame too short: ${bytes.length} bytes`);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const metaLen = view.getUint32(0);
  const meta = JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + metaLen))) as FrameMeta;
  return { meta, jpeg: bytes.slice(4 + metaLen) };
}

export interface WsViewer {
  hello: ViewerHello;
  frames: FramePacket[];
  /** Binary messages seen (mostly frames); count via a counting receiver. */
  rawFrames: { count: number };
  texts: string[];
  closed: Promise<number>;
  send(obj: unknown): void;
  close(): void;
}

/** Connect a viewer socket and resolve once the hello arrives. */
export async function connectViewer(baseUrl: string, seatId: string): Promise<WsViewer> {
  const texts: string[] = [];
  const frames: FramePacket[] = [];
  let helloResolve: (h: ViewerHello) => void;
  const hello = new Promise<ViewerHello>((r) => {
    helloResolve = r;
  });
  let closedResolve: (code: number) => void;
  const closed = new Promise<number>((r) => {
    closedResolve = r;
  });
  const rawFrames = { count: 0 };
  const ws = new WebSocket(`${baseUrl}/ws/${seatId}`);
  ws.binaryType = 'arraybuffer';
  ws.addEventListener('message', (ev) => {
    const data = ev.data;
    if (typeof data === 'string') {
      texts.push(data);
      let parsed: unknown;
      try {
        parsed = JSON.parse(data);
      } catch {
        return;
      }
      if (typeof parsed === 'object' && parsed !== null && 'type' in parsed && parsed.type === 'hello') {
        helloResolve(asHello(parsed));
      }
      return;
    }
    if (data instanceof ArrayBuffer) {
      rawFrames.count += 1;
      frames.push(parseFramePacket(new Uint8Array(data)));
    }
  });
  ws.addEventListener('close', (ev) => closedResolve(ev.code));
  await waitFor(() => ws.readyState === WebSocket.OPEN, 5000, 'viewer ws open');
  return {
    hello: await hello,
    frames,
    rawFrames,
    texts,
    closed,
    send: (obj: unknown) => ws.send(JSON.stringify(obj)),
    close: () => ws.close(),
  };
}

export { waitFor };
