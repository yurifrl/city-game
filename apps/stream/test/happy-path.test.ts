// Happy-path suite over the in-process fake CDP (no chrome required, fully
// deterministic): pool readiness, HTTP surface, two-viewer claim + first-viewer
// input policy, frame relay, status transitions, seat lifecycle, chrome-log
// filtering, and auto-destroy cleanup.

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { SeatPool, poolEventually } from '../src/pool';
import { CHROME_RAW_LOG_LINES, isInterestingChromeLine } from '../src/chrome-log';
import {
  connectViewer,
  parseFramePacket,
  startStream,
  startUpstream,
  testEnv,
  viewerTemplate,
  waitFor,
} from './helpers';
import { jpegSize } from '../src/jpeg';

let pool: SeatPool;
let stream: { url: string; stop(): void };
let upstream: { url: string; stop(): void };

beforeAll(async () => {
  upstream = await startUpstream('seed-from-upstream');
  pool = new SeatPool(testEnv({ CITY_SEATS: '1' }, upstream.url));
  await pool.start();
  await pool.waitReady();
  stream = await startStream(pool, await viewerTemplate());
});

afterAll(async () => {
  stream?.stop();
  await pool?.stop();
  upstream?.stop();
});

describe('pool readiness', () => {
  test('warm seat boots through the boot gate to ready', async () => {
    const status = await (await fetch(`${stream.url}/api/status.json`)).json();
    expect(status).toEqual({
      seats: [
        {
          id: expect.any(String),
          state: 'idle',
          viewers: 0,
          size: { width: 320, height: 240, dpr: 1 },
          debugPort: expect.any(Number),
        },
      ],
    });
  });
});

describe('http surface', () => {
  test('GET / redirects to the best seat viewer', async () => {
    const res = await fetch(`${stream.url}/view`, { redirect: 'manual' });
    expect(res.status).toBe(302);
    const seatId = pool.list()[0]?.id;
    expect(res.headers.get('location')).toBe(`/view/${seatId}`);
  });

  test('viewer page has seat id and viewport baked in, no placeholders', async () => {
    const seatId = pool.list()[0]?.id;
    if (seatId === undefined) throw new Error('pool has no seats');
    const res = await fetch(`${stream.url}/view/${seatId}`);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toContain('text/html');
    const html = await res.text();
    expect(html).toContain(seatId);
    expect(html).not.toContain('__SEAT_ID__');
    expect(html).not.toContain('__WIDTH__');
    expect(html).toContain('width="320"');
    expect(html).toContain('height="240"');
  });

  test('unknown seat viewer page is 404', async () => {
    const res = await fetch(`${stream.url}/view/seat-nope`);
    expect(res.status).toBe(404);
  });

  test('static assets are served, traversal is not', async () => {
    const js = await fetch(`${stream.url}/static/viewer.js`);
    expect(js.status).toBe(200);
    expect(js.headers.get('content-type')).toContain('javascript');
    const css = await fetch(`${stream.url}/static/viewer.css`);
    expect(css.status).toBe(200);
    const traversal = await fetch(`${stream.url}/static/..%2Fenv.ts`);
    expect(traversal.status).toBe(404);
  });

  test('/api/seed proxies the upstream seed endpoint', async () => {
    const res = await fetch(`${stream.url}/api/seed`);
    expect(res.status).toBe(200);
    expect(res.headers.get('content-type')).toContain('application/json');
    expect(await res.json()).toEqual({ seed: 'seed-from-upstream' });
  });
});

describe('two-viewer claim + input + frames', () => {
  test('first viewer is controller, second is spectator', async () => {
    const seatId = pool.list()[0]!.id;
    const a = await connectViewer(stream.url, seatId);
    expect(a.hello).toEqual({
      type: 'hello',
      role: 'controller',
      seatId,
      viewport: { width: 320, height: 240, dpr: 1 },
    });
    const b = await connectViewer(stream.url, seatId);
    expect(b.hello.role).toBe('spectator');

    // status reflects two viewers → busy
    const status = await (await fetch(`${stream.url}/api/status.json`)).json();
    const seat = status.seats.find((s: { id: string }) => s.id === seatId);
    expect(seat.state).toBe('busy');
    expect(seat.viewers).toBe(2);

    await a.close();
    await b.close();
    await Promise.all([a.closed, b.closed]);
    await poolEventually(pool, () => pool.viewerCount(seatId) === 0, 2000, 'viewers detach');
  });

  test('controller input drives the fake chrome; frames relay to both viewers', async () => {
    const seatId = pool.list()[0]!.id;
    const seat = pool.list()[0]!;
    const a = await connectViewer(stream.url, seatId);
    const b = await connectViewer(stream.url, seatId);
    const inputsBefore = seat.fakeCdp?.inputs.length ?? 0;

    // Inside the 320×240 seat viewport: (288,96) → dpr 1 → device pixels equal.
    a.send({ type: 'input', x: 288, y: 96, buttons: 0, down: false });
    await waitFor(() => (seat.fakeCdp?.inputs.length ?? 0) > inputsBefore, 2000, 'fake chrome got input');
    const dispatched = seat.fakeCdp?.inputs.at(-1);
    expect(dispatched?.method).toBe('Input.dispatchMouseEvent');
    // cell (12,4) * TILE_PX(24) → css 288,96; dpr 1 → device pixels equal
    expect(dispatched?.params).toMatchObject({ x: 288, y: 96 });

    await waitFor(() => a.frames.length > 0 && b.frames.length > 0, 2000, 'both viewers framed');
    const frame = a.frames[0]!;
    expect(frame.jpeg[0]).toBe(0xff);
    expect(frame.jpeg[1]).toBe(0xd8);
    expect(frame.jpeg.length).toBeGreaterThan(0);
    expect(frame.meta.seq).toBeGreaterThan(0);
    expect(frame.meta.format).toBe('jpeg');
    expect(frame.meta.cssWidth).toBe(320);
    expect(frame.meta.cssHeight).toBe(240);
    expect(frame.meta.dpr).toBe(1);
    // Intrinsic JPEG size is whatever the fake emitted (1×1); meta must agree
    // with what jpegSize() sniffs out of the payload itself.
    const size = jpegSize(frame.jpeg);
    expect(size?.w).toBe(frame.meta.w);
    expect(size?.h).toBe(frame.meta.h);
    expect(b.frames[0]!.meta.seq).toBeGreaterThan(0);

    // Spectator input is rejected without crashing the seat: the spectator's
    // move never dispatches; the controller's follow-up does, and it is the
    // only new dispatch (order proves the drop, no timing involved).
    b.send({ type: 'input', x: 0, y: 0, buttons: 0, down: false });
    a.send({ type: 'input', x: 24, y: 24, buttons: 0, down: false });
    await waitFor(() => (seat.fakeCdp?.inputs.length ?? 0) > inputsBefore + 1, 2000, 'controller follow-up dispatched');
    expect(seat.fakeCdp?.inputs.length).toBe(inputsBefore + 2);
    expect(seat.fakeCdp?.inputs.at(-1)?.params).toMatchObject({ x: 24, y: 24 });

    // Malformed payloads are ignored the same way.
    a.send({ type: 'nonsense' });
    a.send('not json at all');
    a.send({ type: 'input', x: 48, y: 48, buttons: 0, down: false });
    await waitFor(() => (seat.fakeCdp?.inputs.length ?? 0) > inputsBefore + 2, 2000, 'post-malformed dispatch');
    expect(seat.fakeCdp?.inputs.length).toBe(inputsBefore + 3);
    expect(seat.fakeCdp?.inputs.at(-1)?.params).toMatchObject({ x: 48, y: 48 });

    await a.close();
    await b.close();
  });

  test('post-open keepalive text frames do not count as input', async () => {
    const seatId = pool.list()[0]!.id;
    const seat = pool.list()[0]!;
    const inputsBefore = seat.fakeCdp?.inputs.length ?? 0;
    const ws = await connectViewer(stream.url, seatId);
    ws.send('ping');
    // A valid controller dispatch proves the channel processes messages; the
    // count then shows the ping added nothing.
    ws.send({ type: 'input', x: 72, y: 72, buttons: 0, down: false });
    await waitFor(() => (seat.fakeCdp?.inputs.length ?? 0) > inputsBefore, 2000, 'keepalive test dispatch');
    expect(seat.fakeCdp?.inputs.length).toBe(inputsBefore + 1);
    expect(seat.fakeCdp?.inputs.at(-1)?.params).toMatchObject({ x: 72, y: 72 });
    ws.close();
  });
});

describe('seat lifecycle', () => {
  test('POST /api/seat boots a new seat; DELETE /api/seat destroys it', async () => {
    const created = await fetch(`${stream.url}/api/seat`, { method: 'POST' });
    expect(created.status).toBe(200);
    const { id } = (await created.json()) as { id: string };
    await poolEventually(pool, () => {
      const seat = pool.list().find((s) => s.id === id);
      return seat !== undefined && seat.ready;
    }, 10_000, 'new seat ready');

    const del = await fetch(`${stream.url}/api/seat?id=${id}`, { method: 'DELETE' });
    expect(del.status).toBe(204);
    expect(pool.list().some((s) => s.id === id)).toBe(false);

    const again = await fetch(`${stream.url}/api/seat?id=${id}`, { method: 'DELETE' });
    expect(again.status).toBe(404);
  });

  test('dead seats are reaped by the janitor (auto-destroy cleanup)', async () => {
    const { id } = await (await fetch(`${stream.url}/api/seat`, { method: 'POST' })).json();
    await poolEventually(pool, () => {
      const seat = pool.list().find((s) => s.id === id);
      return seat !== undefined && seat.ready;
    }, 10_000, 'seat ready before reap');
    const seat = pool.list().find((s) => s.id === id)!;
    // Kill the fake chrome underneath: the seat notices and flips to error.
    await seat.fakeCdp?.close();
    await poolEventually(pool, () => !pool.list().some((s) => s.id === id), 5000, 'janitor reaped dead seat');
  });

  test('seat with no live fake fails to boot and self-cleans', async () => {
    // CITY_STREAM_CHROME pointing at a nonexistent binary via real mode.
    const env = testEnv({
      CITY_STREAM_FAKE: '0',
      CITY_STREAM_CHROME: '/nonexistent/chrome-binary',
      CITY_STREAM_BOOT_TIMEOUT_MS: '2000',
    });
    const failing = new SeatPool(env);
    await expect(failing.createSeat()).rejects.toThrow(/chrome/i);
    // Pool state stays consistent: failed seat is either gone or in error.
    expect(failing.list().every((s) => s.status === 'error')).toBe(true);
    await failing.stop();
  });
});

describe('chrome-log filter', () => {
  test('DevTools and fatal lines pass; GPU noise does not', () => {
    expect(isInterestingChromeLine('[123:456:DevTools listening on ws://127.0.0.1:9222]')).toBe(true);
    expect(isInterestingChromeLine('FATAL:gpu_data_manager_impl_private.cc(1234)] Got bad message')).toBe(true);
    expect(isInterestingChromeLine('[123:456:Check failed: false.]')).toBe(true);
    expect(isInterestingChromeLine('Received signal 11 SEGV_MAPERR')).toBe(true);
    expect(isInterestingChromeLine('[23456:84531:1024/173012.345:ERROR:gpu_init.cc(521)] Passthrough is not supported')).toBe(false);
    expect(isInterestingChromeLine('dbus_bus_get_unique_name: connection closed')).toBe(false);
    expect(isInterestingChromeLine('   ')).toBe(false);
    expect(CHROME_RAW_LOG_LINES).toBeGreaterThan(0);
  });
});

describe('frame packet codec', () => {
  test('parseFramePacket round-trips a meta+jpeg packet', () => {
    const meta = { seq: 7, format: 'jpeg', w: 320, h: 240, cssWidth: 320, cssHeight: 240, dpr: 1, ts: 12345 } as const;
    const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);
    const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
    const packet = new Uint8Array(4 + metaBytes.length + jpeg.length);
    new DataView(packet.buffer).setUint32(0, metaBytes.length);
    packet.set(metaBytes, 4);
    packet.set(jpeg, 4 + metaBytes.length);
    const parsed = parseFramePacket(packet);
    expect(parsed.meta).toEqual(meta);
    expect(Array.from(parsed.jpeg)).toEqual([0xff, 0xd8, 0xff, 0xd9]);
  });
});
