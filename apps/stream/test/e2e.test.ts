// E2E per docs/contracts/multiplayer-stream.md §6: spawns the real game
// server (hermetic CITY_DB tempdir) and the stream seat manager, waits for
// pool readiness, then drives two seats through the multiplayer flow over
// real Chrome and the real wire protocol:
//
//   1. two viewers claim two seats; both clients identify via ?seed=
//   2. viewer A moves its cursor over the board → B receives A's presence
//   3. viewer A arms a catalogue card and hovers → B receives A's drag ghost
//   4. viewer A clicks to place → server accepts; B's replica applies
//      building_placed carrying A's actor as owner
//   5. ownership round-trip: owner persisted in SQLite; a raw second
//      connection demolishes the building; receipts stay idempotent
//
// Skipped cleanly when no chrome binary is available. Screenshots land in
// repo-root shots/ (gitignored) as evidence artifacts.

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { SeatPool } from '../src/pool';
import { loadStreamEnv } from '../src/env';
import { startStream } from './helpers';
import { waitFor } from '../src/ids';
import { CdpClient } from '../src/cdp';

const REPO_ROOT = new URL('../../..', import.meta.url).pathname;
const SHOTS_DIR = `${REPO_ROOT}shots/`;
const CHROME_PATH = discoverChrome();

interface GameServer {
  proc: Bun.Subprocess;
  port: number;
  dbPath: string;
}

function discoverChrome(): string | null {
  const forced = process.env['CITY_STREAM_CHROME']?.trim();
  if (forced !== undefined && forced.length > 0) return forced;
  if (process.platform === 'darwin') {
    return '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  }
  for (const candidate of ['chromium', 'chromium-browser', 'google-chrome']) {
    try {
      const probe = Bun.spawnSync([candidate, '--version'], { stdout: 'pipe', stderr: 'pipe' });
      if (probe.exitCode === 0) return candidate;
    } catch {
      /* try next */
    }
  }
  return null;
}

/** Reserves an ephemeral TCP port by binding and releasing it. */
function reservePort(): number {
  const server = Bun.serve({ port: 0, fetch: () => new Response('ok') });
  const port = server.port ?? 0;
  server.stop(true);
  return port;
}

/** Starts the real city-server on a reserved port with a fresh temp database. */
async function startGameServer(): Promise<GameServer> {
  const dir = await mkdtemp(`${tmpdir()}/city-e2e-`);
  const port = reservePort();
  const proc = Bun.spawn(['cargo', 'run', '-q', '-p', 'city-server'], {
    cwd: REPO_ROOT,
    stdout: 'pipe',
    stderr: 'pipe',
    env: { ...Bun.env, CITY_BIND: `127.0.0.1:${port}`, CITY_DB: `${dir}/city.db` },
  });
  // Drain output so the server never blocks on a full pipe.
  void proc.stdout.getReader().read();
  void proc.stderr.getReader().read();
  await waitFor(
    async () => await fetch(`http://127.0.0.1:${port}/api/seed`).then((r) => r.ok).catch(() => false),
    30_000,
    'game server /api/seed',
  );
  return { proc, port, dbPath: `${dir}/city.db` };
}

interface WireEvent {
  type: string;
  [key: string]: unknown;
}

/**
 * Input driver over the stream service: binary JPEG frames in, viewer input
 * out. Used to drive seats; assertions never read this socket.
 */
class Viewer {
  private readonly ws: WebSocket;
  private readonly opened: Promise<void>;
  private hello: { seatId: string; role: string } | undefined;

  constructor(url: string) {
    const { promise, resolve } = Promise.withResolvers<void>();
    this.opened = promise;
    this.ws = new WebSocket(url);
    this.ws.binaryType = 'arraybuffer';
    this.ws.addEventListener('open', () => resolve());
    this.ws.addEventListener('error', () => console.error(`[viewer ${url}] open error`));
    this.ws.addEventListener('close', (ev) => console.error(`[viewer ${url}] closed code=${(ev as CloseEvent).code}`));
    this.ws.addEventListener('message', (ev) => {
      if (typeof ev.data !== 'string') return;
      try {
        const parsed: unknown = JSON.parse(ev.data);
        if (
          typeof parsed === 'object' && parsed !== null && 'type' in parsed &&
          (parsed as { type: unknown }).type === 'hello'
        ) {
          const v = parsed as { seatId?: unknown; role?: unknown };
          if (typeof v.seatId === 'string' && typeof v.role === 'string') {
            this.hello = { seatId: v.seatId, role: v.role };
          }
        }
      } catch {
        /* non-JSON frame: ignore */
      }
    });
  }

  send(payload: object): void {
    this.ws.send(JSON.stringify(payload));
  }

  input(x: number, y: number, buttons: number, down: boolean): void {
    this.send({ type: 'input', x, y, buttons, down });
  }

  close(): void {
    this.ws.close();
  }

  async open(): Promise<void> {
    await Promise.race([
      this.opened,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error(`viewer open timeout: ${this.ws.url}`)), 10_000),
      ),
    ]);
  }

  /** Seat identity + role as assigned by the service's hello frame. */
  async attached(): Promise<{ seatId: string; role: string }> {
    await waitFor(() => this.hello, 10_000, 'viewer hello');
    return this.hello!;
  }
}

/** First event matching `match`, awaiting until one arrives. */
async function sawEvent(watcher: Judge, match: (event: WireEvent) => boolean, what: string, timeoutMs = 10_000): Promise<WireEvent> {
  await waitFor(() => watcher.events.some(match), timeoutMs, what);
  return watcher.events.find(match)!;
}

/**
 * Read-only game-server connection identifying as an actor and recording
 * every wire frame. Drives no input — assertions only.
 */
class Judge {
  private readonly ws: WebSocket;
  readonly events: WireEvent[] = [];
  private readonly opened: Promise<void>;

  constructor(url: string, actorKey: string) {
    const { promise, resolve } = Promise.withResolvers<void>();
    this.opened = promise;
    this.ws = new WebSocket(url);
    this.ws.addEventListener('open', () => {
      resolve();
      this.send({ type: 'identify', key: actorKey });
    });
    this.ws.addEventListener('error', () => console.error(`[judge ${actorKey}] open error`));
    this.ws.addEventListener('close', (ev) => console.error(`[judge ${actorKey}] closed code=${(ev as CloseEvent).code}`));
    this.ws.addEventListener('message', (ev) => {
      if (typeof ev.data !== 'string') return;
      try {
        const parsed: unknown = JSON.parse(ev.data);
        if (typeof parsed === 'object' && parsed !== null && 'type' in parsed) {
          this.events.push(parsed as WireEvent);
        }
      } catch {
        /* undecodable frame — not evidence */
      }
    });
  }

  send(payload: object): void {
    this.ws.send(JSON.stringify(payload));
  }

  close(): void {
    this.ws.close();
  }

  async open(): Promise<void> {
    await Promise.race([
      this.opened,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error(`judge open timeout: ${this.ws.url}`)), 10_000),
      ),
    ]);
  }
}

/** The wire's event envelope: `{ type: 'event', event: {...} }`. */
interface Envelope {
  event: Record<string, unknown>;
}

function envelopeOf(candidate: WireEvent): Envelope | null {
  if (candidate.type !== 'event') return null;
  const event: unknown = candidate.event;
  if (typeof event !== 'object' || event === null) return null;
  return { event: event as Record<string, unknown> };
}

const isPresence = (candidate: WireEvent): boolean => envelopeOf(candidate)?.event['type'] === 'presence';

/** Narrowed presence payload of an already-matched presence event. */
function presenceOf(candidate: WireEvent): { actor: string; cell: { x: number; y: number }; kind: string; def_id: string | null } {
  const event = envelopeOf(candidate)!.event;
  const payload = event['presence'];
  if (typeof payload !== 'object' || payload === null) {
    throw new Error(`malformed presence event: ${JSON.stringify(candidate)}`);
  }
  const body = payload as Record<string, unknown>;
  const actor = body['actor'];
  const cell = body['cell'];
  const kind = body['kind'];
  const defId = body['def_id'];
  if (typeof actor !== 'string' || typeof kind !== 'string' || typeof cell !== 'object' || cell === null) {
    throw new Error(`malformed presence event: ${JSON.stringify(candidate)}`);
  }
  const x = (cell as Record<string, unknown>)['x'];
  const y = (cell as Record<string, unknown>)['y'];
  if (typeof x !== 'number' || typeof y !== 'number') {
    throw new Error(`malformed presence cell: ${JSON.stringify(candidate)}`);
  }
  return { actor, kind, cell: { x, y }, def_id: typeof defId === 'string' ? defId : null };
}

const resultOf = (event: WireEvent): { result: string; command_id: string } | null => {
  if (event.type !== 'command_result') return null;
  const result: unknown = event.result;
  if (typeof result !== 'object' || result === null) return null;
  const record = result as Record<string, unknown>;
  const outcome = record['result'];
  const commandId = record['command_id'];
  return typeof outcome === 'string' && typeof commandId === 'string' ? { result: outcome, command_id: commandId } : null;
};

const isDemolishAccepted = (event: WireEvent): boolean =>
  resultOf(event)?.result === 'accepted' && resultOf(event)!.command_id === 'e2e-demolish-1';

const isBuildingPlaced = (candidate: WireEvent): boolean => envelopeOf(candidate)?.event['type'] === 'building_placed';

/** Narrowed building payload of an already-matched building_placed event. */
function placedBuildingOf(candidate: WireEvent): { id: string; owner: string } {
  const event = envelopeOf(candidate)!.event;
  const building = event['building'];
  if (typeof building !== 'object' || building === null) {
    throw new Error(`malformed building_placed: ${JSON.stringify(candidate)}`);
  }
  const record = building as Record<string, unknown>;
  const id = record['id'];
  const owner = record['owner'];
  if (typeof id !== 'string' || typeof owner !== 'string') {
    throw new Error(`malformed building_placed payload: ${JSON.stringify(candidate)}`);
  }
  return { id, owner };
}

// ---------- CDP helpers over a seat's debug port ----------

async function pageClient(debugPort: number): Promise<CdpClient> {
  const list = (await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json()) as Array<{
    type: string;
    webSocketDebuggerUrl: string;
  }>;
  const page = list.find((target) => target.type === 'page');
  if (page === undefined) throw new Error(`seat ${debugPort}: no page target`);
  return CdpClient.connect(page.webSocketDebuggerUrl);
}

interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

const ORTHO_VIEW_HEIGHT = 96; // initialViewHeight in apps/web camera rig
const BOARD_CENTER = 48; // camera focus is world (48, 0, 48)

/**
 * Maps a board cell to seat-viewport CSS pixels by inverting the client's
 * ortho isometric projection (camera.ts): 45°-yaw / 35.264°-elevation camera
 * at focus (48, 0, 48), frustum height 96 world units, width = height ×
 * aspect. Ground-plane basis: screen-x along (1,0,1)/√2, screen-y along
 * (-1,2,1)/√6, so a cell (x, y) → world (x+0.5, 0, y+0.5) projects to
 *   sx = (x + y − 95)/√2   (screen right)
 *   sy = (y − x)/√6        (screen up)
 */
function cellToPixel(cell: { x: number; y: number }, stage: Rect): { x: number; y: number } {
  const halfH = ORTHO_VIEW_HEIGHT / 2;
  const aspect = stage.width / Math.max(1, stage.height);
  const halfW = halfH * aspect;
  const sx = (cell.x + cell.y - (2 * BOARD_CENTER - 1)) / Math.SQRT2;
  const sy = (cell.y - cell.x) / Math.sqrt(6);
  const px = stage.left + (stage.width * (sx + halfW)) / (2 * halfW);
  const py = stage.top + (stage.height * (halfH - sy)) / (2 * halfH);
  return {
    x: Math.max(1, Math.min(stage.width - 1, Math.round(px))),
    y: Math.max(1, Math.min(stage.height - 1, Math.round(py))),
  };
}

function clickAt(viewer: Viewer, point: { x: number; y: number }): void {
  viewer.input(point.x, point.y, 1, true);
  viewer.input(point.x, point.y, 0, false);
}

function moveTo(viewer: Viewer, point: { x: number; y: number }): void {
  viewer.input(point.x, point.y, 0, false);
}

/**
 * Integration sleep: bridges physical CDP input latency and the screencast
 * cadence of a real headless chrome — no in-process clock to fake.
 */
const settle = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

async function sqlite(dbPath: string, query: string): Promise<Array<Record<string, string | null>>> {
  const proc = Bun.spawnSync(['sqlite3', '-json', dbPath, query], { stdout: 'pipe', stderr: 'pipe' });
  if (proc.exitCode !== 0) throw new Error(`sqlite3 failed: ${proc.stderr.toString()}`);
  const text = proc.stdout.toString().trim();
  return text.length === 0 ? [] : (JSON.parse(text) as Array<Record<string, string | null>>);
}

// ---------- the suite ----------

describe.skipIf(CHROME_PATH === null)('multiplayer e2e (contract §6)', () => {
  let server: GameServer;
  let pool: SeatPool;
  let stream: { url: string; stop(): void };
  const shots: string[] = [];

  beforeAll(async () => {
    server = await startGameServer();
    await mkdir(SHOTS_DIR, { recursive: true });
    pool = new SeatPool(
      loadStreamEnv({
        CITY_STREAM_FAKE: '',
        CITY_STREAM_CHROME: CHROME_PATH!,
        CITY_SEATS: '2',
        CITY_STREAM_WIDTH: '1280',
        CITY_STREAM_HEIGHT: '800',
        CITY_STREAM_DPR: '1',
        CITY_STREAM_QUALITY: '60',
        CITY_STREAM_BOOT_TIMEOUT_MS: '30000',
        CITY_STREAM_JANITOR_MS: '5000',
        CITY_STREAM_PORT: String(reservePort()),
        CITY_GAME_URL: `http://127.0.0.1:${server.port}`,
      }),
    );
    await pool.start();
    await pool.waitReady(60_000);
    stream = await startStream(pool, '<!doctype html><title>viewer</title>');
  }, 120_000);

  afterAll(async () => {
    stream?.stop();
    await pool?.stop();
    server?.proc.kill();
    if (shots.length > 0) console.log(`[e2e] screenshots: ${shots.join(', ')}`);
  }, 20_000);

  test('presence, drag ghost, placed ownership, persisted owner, idempotent receipts', async () => {
    // --- two viewers claim two distinct seats; the first WS on a seat is its
    // controller (the direct pool.attachViewer() path would register the WS as
    // a second, spectator viewer and silently drop its input) ---
    const idle = pool.status().seats.filter((s) => s.state === 'idle');
    expect(idle.length).toBeGreaterThanOrEqual(2);
    const seatA = idle[0]!.id;
    const seatB = idle[1]!.id;

    const viewerA = new Viewer(`${stream.url.replace(/^http/, 'ws')}/ws/${seatA}`);
    const viewerB = new Viewer(`${stream.url.replace(/^http/, 'ws')}/ws/${seatB}`);
    console.log('[e2e] step: viewer connect');
    await Promise.all([viewerA.open(), viewerB.open()]);
    const attachA = await viewerA.attached();
    const attachB = await viewerB.attached();
    console.log('[e2e] step: viewers open');
    expect(attachA.seatId).toBe(seatA);
    expect(attachB.seatId).toBe(seatB);
    expect(attachA.seatId).not.toBe(attachB.seatId);
    expect(attachA.role).toBe('controller');
    expect(attachB.role).toBe('controller');

    // Assertions read the game wire directly, never the stream socket.
    const judgeB = new Judge(`ws://127.0.0.1:${server.port}/ws`, 'e2e-observer-b');
    await judgeB.open();
    console.log('[e2e] step: judge open');
    await waitFor(() => judgeB.events.some((e) => e.type === 'helloworld'), 10_000, 'judge identified');
    console.log('[e2e] step: judge identified');

    const status = pool.status();
    const portA = status.seats.find((s) => s.id === attachA.seatId)!.debugPort!;
    const portB = status.seats.find((s) => s.id === attachB.seatId)!.debugPort!;
    console.log('[e2e] step: cdp connect');
    const cdpA = await pageClient(portA);
    const cdpB = await pageClient(portB);
    console.log('[e2e] step: cdp connected');
    const shot = async (cdp: CdpClient, name: string): Promise<void> => {
      const result = await cdp.send<{ data: string }>('Page.captureScreenshot', { format: 'png' });
      const path = `${SHOTS_DIR}${name}`;
      await Bun.write(path, Uint8Array.from(atob(result.data), (c) => c.charCodeAt(0)));
      shots.push(name);
    };

    try {
      // Both clients must identify through ?seed= and render the world.
      await waitFor(
        async () =>
          (await evaluate(cdpA, "document.getElementById('identity-chip').textContent.startsWith('you: ')")) &&
          (await evaluate(cdpB, "document.getElementById('identity-chip').textContent.startsWith('you: ')")),
        20_000,
        'both seats identified',
      );
      console.log('[e2e] step: both identified');
      // The temp DB seeds exactly 3 definitions and no buildings; placement
      // below creates the first building, verified against SQLite afterwards.
      const seedCount = await sqlite(server.dbPath, 'SELECT COUNT(*) AS n FROM buildings');
      expect(Number(seedCount[0]!.n)).toBe(0);

      const stageA = JSON.parse(
        await evaluate<string>(cdpA, "JSON.stringify(document.getElementById('stage').getBoundingClientRect())"),
      ) as Rect;
      const cardA = JSON.parse(
        await evaluate<string>(
          cdpA,
          "JSON.stringify(document.querySelector('#catalogue .def-card').getBoundingClientRect())",
        ),
      ) as Rect;

      // --- viewer A sweeps the cursor across the board ---
      // Each sample is acknowledged by the judge receiving a presence event
      // for THAT cell, not by a clock: move → observe → move on.
      const sweep = [{ x: 12, y: 12 }, { x: 34, y: 26 }, { x: 56, y: 42 }];
      let cursorEvent: WireEvent | undefined;
      for (const cell of sweep) {
        moveTo(viewerA, cellToPixel(cell, stageA));
        cursorEvent = await sawEvent(
          judgeB,
          (e) => {
            if (!isPresence(e)) return false;
            const p = presenceOf(e);
            return p.cell.x === cell.x && p.cell.y === cell.y;
          },
          `presence sample at cell (${cell.x}, ${cell.y})`,
        );
      }

      // The judge sees A's cursor presence; seat B renders it (screenshot).
      const presence = presenceOf(cursorEvent!);
      expect(presence.kind).toBe('idle');
      expect(presence.actor).not.toBe('e2e-observer-b');
      await settle(600); // screencast latency before the next shot
      await shot(cdpB, '30-cursor-of-A-visible-on-B.png');

      // --- viewer A arms the first catalogue card, then hovers an empty ---
      // --- board cell: armed hover ⇒ drag-ghost presence on B           ---
      const hoverCell = { x: 58, y: 58 };
      const cardPoint = {
        x: Math.round(cardA.left + cardA.width / 2),
        y: Math.round(cardA.top + cardA.height / 2),
      };
      moveTo(viewerA, cardPoint);
      await settle(120); // CDP mousePressed/mouseReleased need distinct events
      clickAt(viewerA, cardPoint);
      // Arming is observable: the next hover input carries the def id.
      moveTo(viewerA, cellToPixel(hoverCell, stageA));
      const dragEvent = await sawEvent(judgeB, (e) => isPresence(e) && presenceOf(e).kind === 'drag', "wire carries A's drag ghost");
      expect(presenceOf(dragEvent).def_id).not.toBeNull();
      await settle(600);
      await shot(cdpB, '40-drag-ghost-of-A-on-B.png');

      // --- viewer A clicks to place (press/release at the same point) ---
      clickAt(viewerA, cellToPixel(hoverCell, stageA));
      const placedEvent = await sawEvent(judgeB, isBuildingPlaced, "wire carries A's building_placed");
      const building = placedBuildingOf(placedEvent);
      expect(building.owner).toBe(presence.actor);
      await settle(600);
      await shot(cdpB, '50-building-placed-from-A-seen-on-B.png');

      // --- ownership round-trip in SQLite ---
      const rows = await sqlite(server.dbPath, `SELECT id, owner FROM buildings WHERE id = ${Number(building.id)}`);
      expect(rows).toHaveLength(1);
      expect(rows[0]!.owner).toBe(building.owner);

      // --- a raw second connection demolishes it ---
      const seedRes = (await (await fetch(`http://127.0.0.1:${server.port}/api/seed`)).json()) as { seed: string };
      const demolisher = new Judge(`ws://127.0.0.1:${server.port}/ws`, seedRes.seed);
      await demolisher.open();
      // The demolish command must land AFTER the runner registered the actor;
      // waiting for the judge's own helloworld guarantees that ordering.
      await waitFor(() => demolisher.events.some((e) => e.type === 'helloworld'), 10_000, 'demolisher identified');
      demolisher.send({
        type: 'command',
        command: { type: 'demolish_building', command_id: 'e2e-demolish-1', building_id: building.id },
      });
      await sawEvent(demolisher, isDemolishAccepted, 'demolish result');
      const after = await sqlite(server.dbPath, `SELECT id, owner FROM buildings WHERE id = ${Number(building.id)}`);
      expect(after).toHaveLength(0);

      // --- receipts are idempotent: replaying the command id replays the
      // stored accepted outcome instead of erroring or double-demolishing.
      // The replay must ADD a second accepted result for the same id —
      // counting proves this event is fresh, not a stale match of the first. ---
      const acceptedBefore = demolisher.events.filter(isDemolishAccepted).length;
      demolisher.send({
        type: 'command',
        command: { type: 'demolish_building', command_id: 'e2e-demolish-1', building_id: building.id },
      });
      await waitFor(
        () => demolisher.events.filter(isDemolishAccepted).length === acceptedBefore + 1,
        10_000,
        'replayed demolish result (fresh second receipt)',
      );
      demolisher.close();
    } finally {
      viewerA.close();
      viewerB.close();
      judgeB.close();
      await cdpA.close();
      await cdpB.close();
    }
  }, 180_000);
});

/** Runtime.evaluate helper returning the value of `expression`. */
async function evaluate<T>(cdp: CdpClient, expression: string): Promise<T> {
  const result = await cdp.send<{ result: { value?: unknown } }>('Runtime.evaluate', { expression, returnByValue: true });
  return result.result.value as T;
}
