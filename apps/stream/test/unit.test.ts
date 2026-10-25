// Pure unit tests: JPEG SOF sniffing, env parsing, ids/helpers, CDP client
// wire framing against an in-process ws server.

import { describe, expect, test } from 'bun:test';
import { jpegSize } from '../src/jpeg';
import { clamp, newSeatId, sleep, waitFor } from '../src/ids';
import { loadStreamEnv } from '../src/env';
import { CdpClient } from '../src/cdp';

describe('jpegSize', () => {
  test('reads dimensions from a synthetic baseline JPEG', () => {
    // SOI + DQT(67) + SOF0: precision 8, 240 high, 320 wide.
    const bytes = new Uint8Array([
      0xff, 0xd8,
      0xff, 0xdb, 0x00, 0x43, ...new Array<number>(67).fill(0),
      0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xf0, 0x01, 0x40, 0x03,
      0xff, 0xd9,
    ]);
    expect(jpegSize(bytes)).toEqual({ w: 320, h: 240 });
  });

  test('rejects non-JPEG and truncated buffers', () => {
    expect(jpegSize(new Uint8Array([0x00, 0x01]))).toBeNull();
    expect(jpegSize(new Uint8Array([0xff, 0xd8]))).toBeNull();
    expect(jpegSize(new Uint8Array(0))).toBeNull();
  });

  test('handles progressive SOF2', () => {
    const bytes = new Uint8Array([0xff, 0xd8, 0xff, 0xc2, 0x00, 0x11, 0x08, 0x01, 0x00, 0x02, 0x00, 0x03]);
    expect(jpegSize(bytes)).toEqual({ w: 512, h: 256 });
  });
});

describe('ids helpers', () => {
  test('newSeatId is prefixed and unique', () => {
    const a = newSeatId();
    const b = newSeatId();
    expect(a).toMatch(/^seat-/);
    expect(a).not.toBe(b);
  });

  test('clamp bounds', () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(99, 0, 10)).toBe(10);
  });

  test('sleep resolves', async () => {
    // sleep() is the helper under test; a real 1ms platform delay is the behavior.
    await sleep(1);
  });

  test('waitFor returns the truthy value and times out with the label', async () => {
    expect(await waitFor(() => 'ok', 100, 'fine')).toBe('ok');
    await expect(waitFor(() => false, 50, 'never-happens', 10)).rejects.toThrow(/never-happens/);
  });
});

describe('loadStreamEnv', () => {
  test('defaults are sane', () => {
    const env = loadStreamEnv({});
    expect(env).toEqual({
      port: 8092,
      gameUrl: 'http://127.0.0.1:8090',
      seats: 2,
      width: 1280,
      height: 800,
      dpr: 1,
      quality: 60,
      chromePath: expect.any(String),
      fake: false,
      bootTimeoutMs: 30_000,
      janitorMs: 5_000,
    });
  });

  test('parses and clamps overrides, truthy fake flags', () => {
    const env = loadStreamEnv({
      CITY_STREAM_PORT: '99999', // above max → clamped
      CITY_SEATS: '7',
      CITY_STREAM_QUALITY: '0', // below min → clamped
    });
    expect(env.port).toBe(65_535);
    expect(env.seats).toBe(7);
    expect(env.quality).toBe(1);
  });

  test('garbage integers throw', () => {
    expect(() => loadStreamEnv({ CITY_STREAM_QUALITY: 'abc' })).toThrow(/CITY_STREAM_QUALITY/);
  });

  test('fake accepts 1/true/yes/on', () => {
    for (const value of ['1', 'true', 'YES', 'on']) {
      expect(loadStreamEnv({ CITY_STREAM_FAKE: value }).fake).toBe(true);
    }
    expect(loadStreamEnv({ CITY_STREAM_FAKE: '0' }).fake).toBe(false);
    expect(loadStreamEnv({ CITY_STREAM_FAKE: '' }).fake).toBe(false);
  });
});

describe('CdpClient', () => {
  test('send correlates responses by id and surfaces events', async () => {
    const server = Bun.serve({
      port: 0,
      fetch: (req, srv) => {
        if (srv.upgrade(req)) return undefined;
        return new Response('ws only', { status: 426 });
      },
      websocket: {
        message(ws, raw) {
          const msg = JSON.parse(String(raw)) as { id?: number; method?: string };
          if (typeof msg.id === 'number') {
            ws.send(JSON.stringify({ id: msg.id, result: { echoed: msg.method } }));
            // Unsolicited event interleaved after the response.
            ws.send(JSON.stringify({ method: 'Page.screencastFrame', params: { data: 'x' } }));
          }
        },
      },
    });
    const cdp = await CdpClient.connect(`ws://127.0.0.1:${server.port}`);
    const events: string[] = [];
    cdp.on('Page.screencastFrame', () => events.push('Page.screencastFrame'));
    cdp.on('Runtime.consoleAPICalled', () => events.push('ignored-other'));
    const result = await cdp.send<{ echoed: string }>('Page.navigate', { url: 'about:blank' });
    expect(result.echoed).toBe('Page.navigate');
    await waitFor(() => events.includes('Page.screencastFrame'), 1000, 'event delivered');
    expect(events).not.toContain('ignored-other');
    cdp.close();
    server.stop(true);
  });

  test('send rejects after close', async () => {
    const server = Bun.serve({
      port: 0,
      fetch: (req, srv) => (srv.upgrade(req) ? undefined : new Response(null, { status: 426 })),
      websocket: { message: () => {} },
    });
    const cdp = await CdpClient.connect(`ws://127.0.0.1:${server.port}`);
    cdp.close();
    await expect(cdp.send('Page.enable')).rejects.toThrow(/closed/i);
    server.stop(true);
  });
});
