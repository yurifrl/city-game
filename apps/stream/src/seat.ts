// One seat = one headless chrome rendering the game client, driven over CDP.
// Boot: spawn chrome → parse DevTools endpoint from stderr → open the game tab
// → enable screencast → wait for the first JPEG frame. Frames fan out to
// subscribers (the HTTP service) as they arrive; viewer input is translated to
// Input.dispatchMouseEvent commands in device pixels.

import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CdpClient } from './cdp';
import { CHROME_RAW_LOG_LINES, isInterestingChromeLine } from './chrome-log';
import { startFakeCdp, type FakeCdp } from './fake-cdp';
import { jpegSize } from './jpeg';
import { clamp, sleep, waitFor } from './ids';
import type { StreamEnv } from './env';
import type { ViewerInput } from './types';

/** A decoded screencast frame ready to fan out to viewers. */
export interface Frame {
  jpeg: Uint8Array;
  seq: number;
  /** JPEG intrinsic (device) pixel size. */
  w: number;
  h: number;
  ts: number;
}

/** Internal seat lifecycle (wire state is computed by the pool). */
export type SeatRuntimeStatus = 'booting' | 'ready' | 'error' | 'stopped';

/** Diagnostics for GET /api/status.json, filled in by the pool. */
export interface SeatRuntime {
  id: string;
  status: SeatRuntimeStatus;
  error?: string;
  pid?: number;
  debugPort?: number;
}

/** A spawned chrome (or fake stand-in) exposing its page-level ws endpoint. */
export interface LaunchedChrome {
  /** Resolves with the page-level DevTools ws URL once the tab exists. */
  pageWsUrl: Promise<string>;
  pid?: number;
  /** DevTools HTTP port (extracted from the endpoint URL). */
  debugPort?: number;
  /** Last N raw stderr lines, for post-mortem when a boot fails. */
  rawLog(): string[];
  stop(): Promise<void>;
}

const DEVTOOLS_LINE = /\bDevTools listening on (ws:\/\/\S+)/;

/**
 * Canonical headless-WebGPU flag set. `--headless=new` and
 * `--remote-debugging-port=0` are load-bearing (the port is parsed from the
 * announced endpoint); the rest keeps the game's renderer happy without a GPU
 * process crash loop.
 */
export function chromeArgs(env: StreamEnv, profileDir: string): string[] {
  const darwin = process.platform === 'darwin';
  const linux = process.platform === 'linux';
  return [
    '--headless=new',
    '--remote-debugging-port=0',
    `--user-data-dir=${profileDir}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    '--enable-unsafe-webgpu',
    // ANGLE backend follows the host: Metal on darwin/arm64; in containers
    // there is no GPU and no Metal, so rendering goes through SwiftShader
    // (which Chrome ≥132 requires an explicit opt-in for).
    `--use-angle=${darwin ? 'metal' : 'swiftshader'}`,
    ...(linux
      ? ['--enable-unsafe-swiftshader', '--no-sandbox', '--disable-dev-shm-usage']
      : []),
    '--mute-audio',
    '--hide-scrollbars',
    `--window-size=${env.width},${env.height}`,
    'about:blank',
  ];
}

/** Line pump for a subprocess stream, invoking onLine per complete line. */
async function pumpLines(stream: ReadableStream<Uint8Array>, onLine: (line: string) => void): Promise<void> {
  const decoder = new TextDecoder();
  let buf = '';
  const reader = stream.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let idx = buf.indexOf('\n');
    while (idx >= 0) {
      onLine(buf.slice(0, idx).replace(/\r$/, ''));
      buf = buf.slice(idx + 1);
      idx = buf.indexOf('\n');
    }
  }
  if (buf !== '') onLine(buf);
}

function readWebSocketDebuggerUrl(tab: unknown): string | null {
  const candidates = Array.isArray(tab) ? tab : [tab];
  for (const candidate of candidates) {
    if (typeof candidate === 'object' && candidate !== null && 'webSocketDebuggerUrl' in candidate) {
      const url = candidate.webSocketDebuggerUrl;
      if (typeof url === 'string') return url;
    }
  }
  return null;
}

function primaryButton(mask: number): 'left' | 'right' | 'middle' {
  if (mask & 2) return 'right';
  if (mask & 4) return 'middle';
  return 'left';
}

/** Launch real chrome and hand back the game tab's ws endpoint. */
export function makeRealLauncher(env: StreamEnv, log: (line: string) => void): () => Promise<LaunchedChrome> {
  return async (): Promise<LaunchedChrome> => {
    const profileDir = await mkdtemp(join(tmpdir(), 'city-stream-seat-'));
    const raw: string[] = [];
    let devtoolsUrl: string | null = null;
    let exitCode: number | null = null;
    const onLine = (line: string): void => {
      raw.push(line);
      if (raw.length > CHROME_RAW_LOG_LINES) raw.shift();
      const announced = /DevTools listening on (ws:\/\/\S+)/.exec(line);
      if (announced !== null && announced[1] !== undefined) devtoolsUrl = announced[1];
      if (isInterestingChromeLine(line)) log(`chrome: ${line}`);
    };

    const proc = Bun.spawn([env.chromePath, ...chromeArgs(env, profileDir)], {
      stdin: 'ignore',
      stdout: 'ignore',
      stderr: 'pipe',
    });
    void pumpLines(proc.stderr, onLine);
    void proc.exited.then((code) => {
      exitCode = code;
    });

    try {
      const browserWs = await waitFor(() => {
        if (devtoolsUrl !== null) return devtoolsUrl;
        if (exitCode !== null) {
          throw new Error(`chrome exited (code ${exitCode}) before announcing DevTools`);
        }
        return null;
      }, env.bootTimeoutMs, 'chrome DevTools endpoint');
      const port = Number(new URL(browserWs).port);
      // NOTE: Chrome ignores a `url` query param here; the seat navigates via
      // CDP instead (Page.navigate), which is also what primes the screencast.
      const res = await fetch(`http://127.0.0.1:${port}/json/new`, {
        method: 'PUT',
      });
      if (!res.ok) throw new Error(`/json/new failed: HTTP ${res.status}`);
      const wsUrl = readWebSocketDebuggerUrl(await res.json());
      if (wsUrl === null) throw new Error('/json/new response missing webSocketDebuggerUrl');

      return {
        pageWsUrl: Promise.resolve(wsUrl),
        pid: proc.pid,
        debugPort: port,
        rawLog: () => raw.slice(),
        stop: async () => {
          try {
            proc.kill();
          } catch {
            /* already gone */
          }
          const reaper = setTimeout(() => {
            try {
              proc.kill(9);
            } catch {
              /* already gone */
            }
          }, 2_000);
          await Promise.race([proc.exited, sleep(2_500)]);
          clearTimeout(reaper);
          await rm(profileDir, { recursive: true, force: true });
        },
      };
    } catch (err) {
      // Never returned, so the caller has no stop() — self-clean here.
      try {
        proc.kill(9);
      } catch {
        /* already gone */
      }
      void rm(profileDir, { recursive: true, force: true });
      const tail = raw.slice(-10).join(' | ');
      const message = err instanceof Error ? err.message : String(err);
      throw new Error(`${message}; chrome log tail: ${tail || '(empty)'}`);
    }
  };
}

export class Seat {
  readonly id: string;
  status: SeatRuntimeStatus = 'booting';
  /** Boot/runtime failure description, stable once set. */
  error?: string;
  /** Latest frame (also the boot gate: ready == first frame seen). */
  lastFrame: Frame | null = null;

  private readonly env: StreamEnv;
  private readonly frameCbs = new Set<(frame: Frame) => void>();
  private cdp?: CdpClient;
  private chrome?: LaunchedChrome;
  /** Fake endpoint when running with CITY_STREAM_FAKE=1 (test inspection). */
  fakeCdp?: FakeCdp;
  private seq = 0;
  private lastButtons = 0;
  /** Per-seat identity UUID claimed from the game server (contract §1). */
  private seed: string | null = null;

  constructor(id: string, env: StreamEnv) {
    this.id = id;
    this.env = env;
  }

  get ready(): boolean {
    return this.status === 'ready';
  }

  /** Subscribe to frames; returns an unsubscribe fn. */
  onFrame(cb: (frame: Frame) => void): () => void {
    this.frameCbs.add(cb);
    return () => {
      this.frameCbs.delete(cb);
    };
  }

  /** Spawn chrome, attach CDP, start screencast, wait for the first frame. */
  async boot(): Promise<void> {
    try {
      let launched: LaunchedChrome;
      if (this.env.fake) {
        const fake = await startFakeCdp({ width: this.env.width * this.env.dpr, height: this.env.height * this.env.dpr });
        this.fakeCdp = fake;
        launched = {
          pageWsUrl: Promise.resolve(fake.pageWsUrl),
          debugPort: fake.port,
          rawLog: () => ['fake-cdp'],
          stop: () => fake.close(),
        };
      } else {
        launched = await makeRealLauncher(this.env, (line) => console.log(`[seat ${this.id}] ${line}`))();
      }
      this.chrome = launched;

      const wsUrl = await launched.pageWsUrl;
      const cdp = await CdpClient.connect(wsUrl);
      this.cdp = cdp;
      cdp.setOnClose((reason) => {
        void this.die(`cdp: ${reason}`);
      });

      await cdp.send('Page.enable');
      await cdp.send('Runtime.enable');
      await cdp.send('Emulation.setDeviceMetricsOverride', {
        width: this.env.width,
        height: this.env.height,
        deviceScaleFactor: this.env.dpr,
        mobile: false,
      });
      // Screencast BEFORE navigate: chrome pushes the current (blank) frame on
      // start, and a static page paints only once — attaching after that paint
      // would never produce a frame. Navigate then forces a fresh composite.
      cdp.on('Page.screencastFrame', (params) => this.onScreencastFrame(params));
      await cdp.send('Page.startScreencast', {
        format: 'jpeg',
        quality: this.env.quality,
        maxWidth: this.env.width * this.env.dpr,
        maxHeight: this.env.height * this.env.dpr,
        everyNthFrame: 1,
      });
      if (this.env.fake) {
        // The fake tab has no URL concept; nothing to navigate.
      } else {
        await this.claimSeed();
        // Contract identity (§1): each seat carries its per-seat UUID in the
        // `?seed=` query; the client sends it as `identify.key`, so actor
        // identity and label resolve server-side and survive reconnects.
        await cdp.send('Page.navigate', { url: this.seedUrl() });
      }

      await waitFor(() => this.lastFrame !== null, this.env.bootTimeoutMs, `seat ${this.id} first frame`);
      this.status = 'ready';
    } catch (err) {
      this.error = err instanceof Error ? err.message : String(err);
      this.status = 'error';
      console.error(`[seat ${this.id}] boot failed: ${this.error}`);
      try {
        const raw = this.chrome?.rawLog() ?? [];
        if (raw.length > 0) console.error(`[seat ${this.id}] last chrome lines:\n  ${raw.slice(-10).join('\n  ')}`);
      } catch {
        /* diagnostics only */
      }
      await this.stop();
      throw err instanceof Error ? err : new Error(String(err));
    }
  }

  /** Game URL with this seat's identity seed (contract §1 identity). */
  private seedUrl(): string {
    return this.seed === null
      ? this.env.gameUrl
      : (() => {
          const url = new URL(this.env.gameUrl);
          url.searchParams.set('seed', this.seed);
          return url.toString();
        })();
  }

  /**
   * Claims the seat's identity UUID from the game server's `/api/seed`
   * (contract §1: one per browser seat). Falls back to the seat id so boots
   * still succeed (plain-dev identity) while the game server is unreachable.
   */
  private async claimSeed(): Promise<void> {
    try {
      const res = await fetch(new URL('/api/seed', `${this.env.gameUrl}/`), {
        signal: AbortSignal.timeout(3_000),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body: unknown = await res.json();
      if (
        typeof body !== 'object' || body === null ||
        !('seed' in body) || typeof body.seed !== 'string' || body.seed.length === 0
      ) {
        throw new Error('response missing seed');
      }
      this.seed = body.seed;
    } catch (err) {
      console.warn(`[seat ${this.id}] seed claim failed (${err instanceof Error ? err.message : err}); using seat id as identity key`);
      this.seed = `seat-${this.id}`;
    }
  }

  /** Forward a viewer input to chrome; throws when the seat is not live. */
  async dispatchInput(input: ViewerInput): Promise<void> {
    const cdp = this.cdp;
    if (cdp === undefined || !this.ready) throw new Error(`seat ${this.id} is not ready`);
    const x = Math.round(clamp(input.x, 0, this.env.width) * this.env.dpr);
    const y = Math.round(clamp(input.y, 0, this.env.height) * this.env.dpr);
    const buttons = clamp(Math.round(input.buttons), 0, 7);
    if (input.kind === 'wheel') {
      await cdp.send('Input.dispatchMouseEvent', {
        type: 'mouseWheel',
        x,
        y,
        deltaX: input.deltaX ?? 0,
        deltaY: input.deltaY ?? 0,
        button: 'none',
        buttons,
        pointerType: 'mouse',
      });
      return;
    }
    const newlyPressed = buttons & ~this.lastButtons;
    const released = this.lastButtons & ~buttons;
    this.lastButtons = buttons;
    if (input.down && newlyPressed !== 0) {
      await cdp.send('Input.dispatchMouseEvent', {
        type: 'mousePressed',
        x,
        y,
        button: primaryButton(newlyPressed),
        buttons,
        clickCount: 1,
        pointerType: 'mouse',
      });
      return;
    }
    if (!input.down && released !== 0) {
      await cdp.send('Input.dispatchMouseEvent', {
        type: 'mouseReleased',
        x,
        y,
        button: primaryButton(released),
        buttons,
        clickCount: 1,
        pointerType: 'mouse',
      });
      return;
    }
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none', buttons, pointerType: 'mouse' });
  }

  /** Tear down CDP + chrome; idempotent. */
  async stop(): Promise<void> {
    this.status = 'stopped';
    const cdp = this.cdp;
    this.cdp = undefined;
    if (cdp !== undefined) {
      cdp.setOnClose(() => {});
      try {
        await cdp.send('Page.stopScreencast');
      } catch {
        /* socket may already be gone */
      }
      cdp.close();
    }
    const chrome = this.chrome;
    this.chrome = undefined;
    if (chrome !== undefined) await chrome.stop();
    this.fakeCdp = undefined;
    this.lastFrame = null;
  }

  /** Pool-facing diagnostics. */
  runtime(): SeatRuntime {
    const rt: SeatRuntime = { id: this.id, status: this.status };
    if (this.error !== undefined) rt.error = this.error;
    if (this.chrome?.pid !== undefined) rt.pid = this.chrome.pid;
    if (this.chrome?.debugPort !== undefined) rt.debugPort = this.chrome.debugPort;
    return rt;
  }

  /** Runtime death (crash/socket close) — mark error and clean up. */
  private async die(reason: string): Promise<void> {
    if (this.status === 'stopped' || this.status === 'error') return;
    this.status = 'error';
    this.error = reason;
    console.error(`[seat ${this.id}] died: ${reason}`);
    await this.stop();
  }

  private onScreencastFrame(params: unknown): void {
    if (typeof params !== 'object' || params === null || !('data' in params)) return;
    const data = params.data;
    if (typeof data !== 'string') return;
    let jpeg: Uint8Array;
    try {
      jpeg = Uint8Array.from(Buffer.from(data, 'base64'));
    } catch {
      return;
    }
    const size = jpegSize(jpeg);
    const frame: Frame = {
      jpeg,
      seq: ++this.seq,
      w: size?.w ?? this.env.width * this.env.dpr,
      h: size?.h ?? this.env.height * this.env.dpr,
      ts: Date.now(),
    };
    this.lastFrame = frame;
    for (const cb of this.frameCbs) cb(frame);
    // Acknowledge so chrome keeps producing; sessionId comes back in params.
    let ackSession: number | undefined;
    if ('sessionId' in params && typeof params.sessionId === 'number') ackSession = params.sessionId;
    void this.cdp?.send('Page.screencastFrameAck', { sessionId: ackSession }).catch(() => {});
  }
}
