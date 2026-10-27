// Regression: a WebSocket whose handshake never completes (stays in
// CONNECTING — no `open`, no `close`) must not wedge the reconnect loop.
// Observed live: a headless seat sat in "connecting…" for 9+ minutes while the
// same server answered fresh handshakes in milliseconds. The client now bounds
// every attempt with a watchdog and falls back to the existing backoff path.
//
// The global WebSocket is replaced with a scriptable double whose readyState
// and event firing are under test control.

import { afterEach, beforeEach, describe, expect, test } from "bun:test";

type WSHandler = ((ev: unknown) => void) | null;

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  static OPEN = 1;
  static CLOSED = 3;
  readyState = 0; // CONNECTING
  onopen: WSHandler = null;
  onclose: WSHandler = null;
  onmessage: WSHandler = null;
  onerror: WSHandler = null;
  closed = false;
  sent: string[] = [];

  constructor(public url: string) {
    FakeWebSocket.instances.push(this);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.closed = true;
  }

  // Test controls — fire the browser's lifecycle events.
  fireOpen(): void {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }

  fireClose(): void {
    this.readyState = 3;
    this.onclose?.(new Event("close"));
  }

  fireMessage(payload: unknown): void {
    this.onmessage?.({ data: JSON.stringify(payload) });
  }
}

const realWebSocket = globalThis.WebSocket;
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;

// Minimal timer double: scheduled work runs only when `advance()` fires it.
interface Scheduled {
  at: number;
  fn: () => void;
  id: number;
}
let scheduled: Scheduled[] = [];
let now = 0;

function schedule(fn: () => void, ms: number): number {
  scheduled.push({ at: now + ms, fn });
  return scheduled.length;
}
function advance(ms: number): void {
  now += ms;
  for (const s of [...scheduled].sort((a, b) => a.at - b.at)) {
    if (s.at <= now) {
      scheduled = scheduled.filter((x) => x !== s);
      s.fn();
    }
  }
}

// The client casts window.setTimeout's return to `number`; ids are unique.
let nextTimerId = 1;
function schedule(fn: () => void, ms: number): number {
  const id = nextTimerId++;
  scheduled.push({ at: now + ms, fn, id });
  return id;
}
function advance(ms: number): void {
  now += ms;
  for (const s of [...scheduled].sort((a, b) => a.at - b.at)) {
    if (s.at <= now) {
      scheduled = scheduled.filter((x) => x !== s);
      s.fn();
    }
  }
}

const clearById = (id: number) => { scheduled = scheduled.filter((s) => s.id !== id); };
// The client casts window.setTimeout's return to `number`; index satisfies it.
const fakeWindow = { setTimeout: schedule, clearTimeout: clearById } as unknown as typeof window;

import { CityClient } from "../src/connection/client";

interface Harness {
  client: CityClient;
  statuses: string[];
  identified: string[];
  hellos: unknown[];
}

function makeClient(key = "test-key"): Harness {
  const statuses: string[] = [];
  const identified: string[] = [];
  const hellos: unknown[] = [];
  const client = new CityClient(
    {
      onIdentified: (you) => identified.push(you),
      onHello: (world) => hellos.push(world),
      onEvent: () => {},
      onStatus: (status) => statuses.push(status),
    },
    "ws://game.test/ws",
    key,
  );
  return { client, statuses, identified, hellos };
}

describe("CityClient connect watchdog", () => {
  beforeEach(() => {
    FakeWebSocket.instances = [];
    scheduled = [];
    now = 0;
    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket;
    globalThis.setTimeout = schedule as unknown as typeof setTimeout;
    globalThis.clearTimeout = clearById as unknown as typeof clearTimeout;
    globalThis.window = fakeWindow;
  });

  afterEach(() => {
    globalThis.WebSocket = realWebSocket;
    globalThis.setTimeout = realSetTimeout;
    globalThis.clearTimeout = realClearTimeout;
    // @ts-expect-error test-only teardown
    delete globalThis.window;
  });

  test("a stalled handshake is abandoned and retried; a later attempt connects", () => {
    const { client, statuses } = makeClient();
    client.connect();
    const stalled = FakeWebSocket.instances[0];
    expect(statuses).toEqual(["connecting"]);

    advance(5_000); // watchdog fires on the stalled socket
    expect(stalled.closed).toBe(true);
    expect(stalled.onopen).toBeNull();
    expect(statuses).toEqual(["connecting"]); // still connecting: retrying

    advance(1_000); // base backoff elapses → second attempt
    const second = FakeWebSocket.instances[1];
    expect(second).toBeDefined();
    second.fireOpen();
    expect(statuses).toEqual(["connecting", "online"]);
    expect(second.sent).toEqual([JSON.stringify({ type: "identify", key: "test-key" })]);
  });

  test("open inside the watchdog window clears the deadline; no spurious close", () => {
    const { client, statuses } = makeClient();
    client.connect();
    const ws = FakeWebSocket.instances[0];
    ws.fireOpen();
    advance(60_000);
    expect(ws.closed).toBe(false); // healthy socket untouched by the dead watchdog
    expect(statuses).toEqual(["connecting", "online"]);
  });

  test("close inside the window takes the normal reconnect path", () => {
    const { client, statuses } = makeClient();
    client.connect();
    const ws = FakeWebSocket.instances[0];
    ws.fireClose();
    advance(5_000);
    expect(ws.closed).toBe(false); // already closed path: no watchdog kill
    const second = FakeWebSocket.instances[1];
    expect(second).toBeDefined(); // backoff retry scheduled by onclose
    expect(statuses).toEqual(["connecting", "offline", "connecting"]);
  });

  test("close() by the user cancels the watchdog (no retry after shutdown)", () => {
    const { client } = makeClient();
    client.connect();
    client.close();
    advance(60_000);
    expect(FakeWebSocket.instances).toHaveLength(1); // no second attempt
  });
});
