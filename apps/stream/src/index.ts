#!/usr/bin/env bun
// city-game stream service — entry point.
//
// Run order:
//   1. game server (crates/server) listening on CITY_GAME_URL (default :8080)
//   2. bun run src/index.ts          → HTTP+WS on CITY_STREAM_PORT (default :8081)
//   3. open http://127.0.0.1:8081/view (redirects to a live seat)
//
// CITY_STREAM_FAKE=1 replaces chrome with an in-process fake CDP server —
// used by tests and for developing without a chrome install.

import { loadStreamEnv } from './env';
import { SeatPool } from './pool';
import { makeService } from './service';
import { loadViewerTemplate } from './assets';

const env = loadStreamEnv(Bun.env);
const pool = new SeatPool(env);
const viewerTemplate = await loadViewerTemplate();
const service = makeService(pool, viewerTemplate);

const server = Bun.serve({
  port: env.port,
  fetch: service.fetch,
  websocket: service.websocket,
});

console.log(
  `[stream] listening on :${server.port} · ${env.seats} warm seat(s) · ` +
    (env.fake ? 'FAKE cdp' : `chrome: ${env.chromePath}`) +
    ` · viewport ${env.width}x${env.height}@${env.dpr} q${env.quality}`,
);

// Boot the warm pool after the listener is up so /api/status.json can report
// `booting` seats while chrome starts.
await pool.start();

process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);

async function shutdown(): Promise<void> {
  console.log('[stream] shutting down');
  server.stop(true);
  await pool.stop();
  process.exit(0);
}
