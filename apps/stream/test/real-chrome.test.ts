// Real headless-chrome integration test.
//
// Skipped cleanly unless a chrome binary is present (or CITY_STREAM_CHROME is
// set). Boots one seat against a trivial local page: this exercises
// makeRealLauncher — process spawn, DevTools endpoint parse from stderr, tab
// enumeration, screencast handshake, input dispatch, teardown.

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { Seat } from '../src/seat';
import { loadStreamEnv } from '../src/env';
import { waitFor } from '../src/ids';

let chromePath: string | null = process.env['CITY_STREAM_CHROME'] || null;
if (chromePath === null) {
  const candidates = [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/usr/bin/google-chrome',
    '/snap/bin/chromium',
  ];
  for (const candidate of candidates) {
    if (await Bun.file(candidate).exists()) {
      chromePath = candidate;
      break;
    }
  }
}

/** Trivial one-page HTTP server so the seat renders without network access. */
async function startPageServer(): Promise<{ url: string; stop(): void }> {
  const server = Bun.serve({
    port: 0,
    fetch: () => new Response('<!doctype html><title>ok</title><h1>up</h1>', { headers: { 'content-type': 'text/html' } }),
  });
  return { url: `http://127.0.0.1:${server.port}`, stop: () => server.stop(true) };
}

describe.skipIf(chromePath === null)('real chrome integration', () => {
  const env = loadStreamEnv({
    CITY_STREAM_FAKE: '0',
    CITY_SEATS: '1',
    CITY_STREAM_WIDTH: '320',
    CITY_STREAM_HEIGHT: '240',
    CITY_STREAM_DPR: '1',
    CITY_STREAM_BOOT_TIMEOUT_MS: '20000',
    ...(chromePath !== null ? { CITY_STREAM_CHROME: chromePath } : {}),
  });
  let seat: Seat;
  let page: { url: string; stop(): void };

  beforeAll(async () => {
    page = await startPageServer();
    env.gameUrl = page.url;
  });

  afterAll(async () => {
    await seat?.stop();
    page?.stop();
  });

  test('seat boots to first frame through real chrome', async () => {
    seat = new Seat(`seat-real-${crypto.randomUUID().slice(0, 8)}`, env);
    await seat.boot();
    expect(seat.ready).toBe(true);
    expect(seat.lastFrame).not.toBeNull();
    expect(seat.lastFrame?.w).toBe(320);
    expect(seat.lastFrame?.h).toBe(240);
    expect(seat.runtime().pid).toBeGreaterThan(0);
  }, 25_000);

  test('input dispatch does not fault real chrome', async () => {
    await seat.dispatchInput({ type: 'input', x: 5, y: 5, buttons: 0, down: false });
    await seat.dispatchInput({ type: 'input', x: 5, y: 5, buttons: 1, down: true });
    await seat.dispatchInput({ type: 'input', x: 6, y: 6, buttons: 0, down: false });
    expect(seat.ready).toBe(true);
  }, 10_000);

  test('stop tears chrome down', async () => {
    const pid = seat.runtime().pid;
    if (pid === undefined) throw new Error('seat reported no chrome pid');
    await seat.stop();
    // Chrome may linger briefly after SIGTERM; poll until it is really gone.
    await waitFor(
      () => {
        try {
          process.kill(pid, 0);
          return false;
        } catch {
          return true;
        }
      },
      5_000,
      'chrome process exit',
    );
  }, 15_000);
});
