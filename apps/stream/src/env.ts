// Environment-driven configuration for the stream service.

export interface StreamEnv {
  /** Listen port of this service. */
  port: number;
  /** Base URL of the game server (crates/server). */
  gameUrl: string;
  /** Warm pool size maintained by the janitor. */
  seats: number;
  /** Seat page viewport (CSS px) and device scale factor. */
  width: number;
  height: number;
  dpr: number;
  /** Screencast JPEG quality (1–100). */
  quality: number;
  /** Chrome/chromium binary used to render seat pages. */
  chromePath: string;
  /** Run against the in-process fake CDP instead of real chrome. */
  fake: boolean;
  /** Max time a seat may take from spawn to first frame. */
  bootTimeoutMs: number;
  /** Janitor pass interval (reap dead seats, top up the warm pool). */
  janitorMs: number;
}

const TRUTHY: Record<string, true> = { '1': true, true: true, yes: true, on: true };

function envInt(env: Record<string, string | undefined>, name: string, def: number, min: number, max: number): number {
  const raw = env[name];
  if (raw === undefined || raw.trim() === '') return def;
  const v = Number.parseInt(raw, 10);
  if (Number.isNaN(v)) throw new Error(`${name} must be an integer, got ${JSON.stringify(raw)}`);
  return Math.min(max, Math.max(min, v));
}

function envStr(env: Record<string, string | undefined>, name: string, def: string): string {
  const raw = env[name];
  return raw !== undefined && raw.trim() !== '' ? raw.trim() : def;
}

function defaultChromePath(): string {
  if (process.platform === 'darwin') {
    return '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  }
  return 'chromium';
}

/** Parse service configuration from an environment map (defaults are sane). */
export function loadStreamEnv(env: Record<string, string | undefined>): StreamEnv {
  const fakeRaw = env['CITY_STREAM_FAKE']?.trim().toLowerCase();
  return {
    port: envInt(env, 'CITY_STREAM_PORT', 8092, 1, 65_535),
    gameUrl: envStr(env, 'CITY_GAME_URL', 'http://127.0.0.1:8090'),
    seats: envInt(env, 'CITY_SEATS', 2, 0, 64),
    width: envInt(env, 'CITY_STREAM_WIDTH', 1280, 160, 3840),
    height: envInt(env, 'CITY_STREAM_HEIGHT', 800, 120, 2160),
    dpr: envInt(env, 'CITY_STREAM_DPR', 1, 1, 3),
    quality: envInt(env, 'CITY_STREAM_QUALITY', 60, 1, 100),
    chromePath: envStr(env, 'CITY_STREAM_CHROME', defaultChromePath()),
    fake: fakeRaw !== undefined && TRUTHY[fakeRaw] === true,
    bootTimeoutMs: envInt(env, 'CITY_STREAM_BOOT_TIMEOUT_MS', 30_000, 1_000, 300_000),
    janitorMs: envInt(env, 'CITY_STREAM_JANITOR_MS', 5_000, 50, 60_000),
  };
}
