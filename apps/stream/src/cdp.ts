// Minimal CDP-over-WebSocket client (flat protocol, sessions supported).
// Just enough for the bridge: Target.createTarget/attachToTarget, Page.enable,
// Page.startScreencast/stopScreencast/screencastFrameAck, Runtime.enable,
// Runtime.evaluate, Emulation.setDeviceMetricsOverride, Input.dispatchMouseEvent.
//
// CDP responses have no schema, so params/results cross this boundary as
// `unknown` and callers narrow what they use.

export type CdpHandler = (params: unknown, sessionId: string | undefined) => void;

interface Pending {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const CALL_TIMEOUT_MS = 15_000;

export class CdpClient {
  private readonly ws: WebSocket;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  private readonly handlers = new Map<string, Set<CdpHandler>>();
  private onCloseCb?: (reason: string) => void;
  /** True once the underlying socket is gone. */
  closed = false;

  private constructor(ws: WebSocket) {
    this.ws = ws;
    ws.addEventListener('message', (ev) => {
      if ('data' in ev) this.onMessage(String(ev.data));
    });
    ws.addEventListener('close', () => {
      this.closed = true;
      this.failAll(new Error('cdp socket closed'));
      this.onCloseCb?.('cdp socket closed');
    });
    ws.addEventListener('error', () => {
      /* a close event always follows */
    });
  }

  /** Connect to a browser/page ws endpoint, rejecting on timeout. */
  static async connect(url: string, timeoutMs = 10_000): Promise<CdpClient> {
    const ws = new WebSocket(url);
    const { promise, resolve, reject } = Promise.withResolvers<CdpClient>();
    const timer = setTimeout(
      () => reject(new Error(`cdp connect timeout after ${timeoutMs}ms: ${url}`)),
      timeoutMs,
    );
    ws.addEventListener('open', () => resolve(new CdpClient(ws)), { once: true });
    ws.addEventListener('error', () => reject(new Error(`cdp connect failed: ${url}`)), {
      once: true,
    });
    try {
      return await promise;
    } finally {
      clearTimeout(timer);
    }
  }

  /** Register a single close callback (last registration wins). */
  setOnClose(cb: (reason: string) => void): void {
    this.onCloseCb = cb;
  }

  /** Subscribe to an event method; returns an unsubscribe fn. */
  on(method: string, cb: CdpHandler): () => void {
    let set = this.handlers.get(method);
    if (!set) {
      set = new Set();
      this.handlers.set(method, set);
    }
    set.add(cb);
    return () => {
      set?.delete(cb);
    };
  }

  /**
   * Fire a CDP command and await its result. `T` is the caller-declared shape
   * of the CDP result (the protocol has no schema to parse against).
   */
  send<T = void>(method: string, params?: object, sessionId?: string): Promise<T> {
    if (this.closed || this.ws.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error(`cdp closed (send ${method})`));
    }
    const id = this.nextId++;
    const { promise, resolve, reject } = Promise.withResolvers<T>();
    const timer = setTimeout(() => {
      this.pending.delete(id);
      reject(new Error(`cdp ${method} timed out after ${CALL_TIMEOUT_MS}ms`));
    }, CALL_TIMEOUT_MS);
    this.pending.set(id, {
      // Caller-supplied response shape; CDP results arrive schema-less.
      resolve: (v: unknown) => resolve(v as T),
      reject,
      timer,
    });
    this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    return promise;
  }

  close(): void {
    if (!this.closed) {
      try {
        this.ws.close();
      } catch {
        /* already gone */
      }
    }
  }

  private failAll(err: Error): void {
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(err);
    }
    this.pending.clear();
  }

  private onMessage(raw: string): void {
    let msg: unknown;
    try {
      msg = JSON.parse(raw);
    } catch {
      return; // not CDP — ignore
    }
    if (typeof msg !== 'object' || msg === null) return;
    const m = msg as {
      id?: unknown;
      method?: unknown;
      params?: unknown;
      sessionId?: unknown;
      result?: unknown;
      error?: unknown;
    };
    if (typeof m.id === 'number') {
      const p = this.pending.get(m.id);
      if (!p) return;
      this.pending.delete(m.id);
      clearTimeout(p.timer);
      if (m.error !== undefined && m.error !== null) {
        p.reject(new Error(`cdp error: ${summarizeCdpError(m.error)}`));
      } else {
        p.resolve(m.result);
      }
      return;
    }
    if (typeof m.method === 'string') {
      const set = this.handlers.get(m.method);
      if (set) {
        const sessionId = typeof m.sessionId === 'string' ? m.sessionId : undefined;
        for (const cb of set) cb(m.params, sessionId);
      }
    }
  }
}

function summarizeCdpError(err: unknown): string {
  if (typeof err === 'object' && err !== null && 'message' in err) {
    const message = err.message;
    if (typeof message === 'string') return message;
  }
  return JSON.stringify(err);
}
