// In-process fake CDP endpoint used for deterministic tests (and for running
// the whole service with CITY_STREAM_FAKE=1 without chrome installed).
//
// Behavior: answers every CDP command with `{}`; turns screencast on/off with
// Page.start/stopScreencast; emits a tiny valid JPEG frame whenever the page
// "damages" (Input.dispatch* or an explicit emitFrame()). Input commands are
// recorded in arrival order for assertions.

import type { LaunchedChrome } from './seat';
import type { ServerWebSocket } from 'bun';

/** A standard 1×1 baseline JPEG (FFD8..FFD9), parsed by jpegSize(). */
export const TINY_JPEG_BASE64 =
  '/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/2wBDAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+iiigD//2Q==';

/** Methods that mark the page as damaged (fake emits a fresh frame after). */
const DAMAGE_METHODS: Record<string, true> = {
  'Input.dispatchMouseEvent': true,
  'Input.dispatchKeyEvent': true,
};

export interface FakeCdp {
  /** HTTP port the fake endpoint listens on. */
  port: number;
  pageWsUrl: string;
  /** Every Input.dispatch* command, { method, params }, in arrival order. */
  inputs: Array<{ method: string; params: unknown }>;
  screencastOn: boolean;
  emitFrame(): void;
  close(): Promise<void>;
}

interface FakeWsData {
  readonly id: number;
}

export async function startFakeCdp(opts?: {
  width?: number;
  height?: number;
}): Promise<FakeCdp> {
  const width = opts?.width ?? 1280;
  const height = opts?.height ?? 800;
  const sockets = new Set<ServerWebSocket<FakeWsData>>();
  const inputs: FakeCdp['inputs'] = [];
  let screencastOn = false;
  let nextId = 1;

  const sendJson = (ws: ServerWebSocket<FakeWsData>, msg: object): void => {
    ws.send(JSON.stringify(msg));
  };

  const emitFrame = (): void => {
    if (!screencastOn) return;
    const params = {
      sessionId: 1,
      data: TINY_JPEG_BASE64,
      metadata: { pageScaleFactor: 1, offsetTop: 0, pageOffsetTop: 0, scrollOffsetX: 0, scrollOffsetY: 0 },
      deviceWidth: width,
      deviceHeight: height,
    };
    for (const ws of sockets) sendJson(ws, { method: 'Page.screencastFrame', params });
  };

  const handleMessage = (ws: ServerWebSocket<FakeWsData>, raw: string | Buffer): void => {
    let msg: unknown;
    try {
      msg = JSON.parse(String(raw));
    } catch {
      return;
    }
    if (typeof msg !== 'object' || msg === null) return;
    if ('method' in msg && typeof msg.method === 'string') {
      const method = msg.method;
      if (method.startsWith('Input.')) inputs.push({ method, params: 'params' in msg ? msg.params : undefined });
      if (method === 'Page.startScreencast') {
        screencastOn = true;
        // Real chrome pushes the current frame immediately on start; the seat
        // boot gate (first frame) depends on it.
        queueMicrotask(emitFrame);
      }
      if (method === 'Page.stopScreencast') screencastOn = false;
      if (DAMAGE_METHODS[method]) queueMicrotask(emitFrame);
    }
    if ('id' in msg && typeof msg.id === 'number') sendJson(ws, { id: msg.id, result: {} });
  };

  const server = Bun.serve<FakeWsData>({
    port: 0,
    fetch(req, srv) {
      const upgraded = srv.upgrade(req, { data: { id: nextId++ } });
      if (upgraded) return undefined;
      return new Response('city-stream fake cdp: websocket endpoint only', { status: 426 });
    },
    websocket: {
      open(ws) {
        sockets.add(ws);
      },
      message(ws, message) {
        handleMessage(ws, message);
      },
      close(ws) {
        sockets.delete(ws);
      },
    },
  });

  // Bun types port as `number | undefined` (unix-socket serves); port 0 here.
  const port = server.port ?? 0;
  return {
    port,
    pageWsUrl: `ws://127.0.0.1:${port}/devtools/page/fake-${port}`,
    inputs,
    get screencastOn() {
      return screencastOn;
    },
    emitFrame,
    close: () => {
      server.stop(true);
      return Promise.resolve();
    },
  };
}
