// WebSocket transport for the command path. Connects to /ws (dev proxy targets
// the server), identifies with a stable actor key on every open (first message
// on a fresh connection, per docs/contracts/multiplayer-stream.md §1.1),
// reconnects with 1s→10s exponential backoff reset on successful open under a
// 5s connect watchdog (a handshake can stall forever with no open/close),
// tracks pending commands by command_id, and dispatches server messages to the
// handlers wired by the composition root.

import type {
  CommandResultTS,
  CommandTS,
  ClientMessageTS,
  EventTS,
  InputMessageTS,
  ServerMessageTS,
  WorldTS,
} from "../protocol/types";

export type ConnectionStatus = "connecting" | "online" | "offline";

export interface ClientHandlers {
  /** Fired on the hello frame — the seat's resolved actor id. */
  onIdentified(you: string): void;
  /** Fired on the helloworld frame — the full world snapshot. */
  onHello(world: WorldTS): void;
  onEvent(event: EventTS): void;
  onStatus(status: ConnectionStatus): void;
}

const BASE_BACKOFF_MS = 1000;
const MAX_BACKOFF_MS = 10000;
/**
 * Wall-clock budget for one connection attempt. A WebSocket whose handshake
 * stalls (network churn, proxy remap, lost upgrade) never fires `open` or
 * `close` on its own — observed as a 9-minute "connecting…" on a headless
 * seat while the same server answered fresh handshakes in milliseconds.
 * Without a deadline the retry loop never wakes up.
 */
const CONNECT_TIMEOUT_MS = 5000;
const KEY_STORAGE_ITEM = "city-key";

/**
 * Resolves the seat's actor key: the `?seed=` UUID when booted by the seat
 * manager, otherwise a random UUID persisted in localStorage for direct dev
 * use. Stable across reloads and reconnects so the server keeps identity and
 * label.
 */
export function resolveIdentityKey(search: string = window.location.search): string {
  const fromQuery = new URLSearchParams(search).get("seed");
  if (fromQuery) return fromQuery;
  let stored = window.localStorage.getItem(KEY_STORAGE_ITEM);
  if (!stored) {
    stored = crypto.randomUUID();
    window.localStorage.setItem(KEY_STORAGE_ITEM, stored);
  }
  return stored;
}

interface PendingEntry {
  resolve(result: CommandResultTS): void;
  reject(reason: Error): void;
}

function defaultUrl(): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}/ws`;
}

export class CityClient {
  private ws: WebSocket | null = null;
  private pending = new Map<string, PendingEntry>();
  private reconnectTimer: number | null = null;
  private connectTimer: number | null = null;
  private backoffAttempt = 0;
  private closedByUser = false;
  private currentStatus: ConnectionStatus = "offline";
  private readonly key: string;

  constructor(
    private readonly handlers: ClientHandlers,
    private readonly url: string = defaultUrl(),
    key: string = resolveIdentityKey(),
  ) {
    this.key = key;
  }

  connect(): void {
    this.closedByUser = false;
    this.open();
  }

  /**
   * Sends a command and resolves with the server's command_result for its
   * command_id. Rejects if the socket is down or drops before the reply.
   */
  send(command: CommandTS): Promise<CommandResultTS> {
    const ws = this.ws;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("not connected to the city server"));
    }
    return new Promise<CommandResultTS>((resolve, reject) => {
      this.pending.set(command.command_id, { resolve, reject });
      this.transmit({ type: "command", command });
    });
  }

  /**
   * Sends a pointer-intent packet. Fire-and-forget: presence packets never
   * produce receipts, so there is nothing to await or retry.
   */
  sendInput(input: InputMessageTS): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    this.transmit({ type: "input", input });
  }

  close(): void {
    this.closedByUser = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.connectTimer !== null) {
      clearTimeout(this.connectTimer);
      this.connectTimer = null;
    }
    this.failPending(new Error("client closed before the server replied"));
    this.ws?.close();
    this.ws = null;
  }

  private transmit(message: ClientMessageTS): void {
    this.ws?.send(JSON.stringify(message));
  }

  private open(): void {
    this.setStatus("connecting");
    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url);
    } catch (err) {
      console.warn("city client: socket creation failed", err);
      this.scheduleReconnect();
      return;
    }
    this.ws = ws;
    // A WebSocket whose handshake stalls stays in CONNECTING forever — no
    // `open`, no `close` — so the reconnect loop never fires. Arm a one-shot
    // deadline per attempt; a healthy server answers in milliseconds.
    this.connectTimer = window.setTimeout(() => {
      this.connectTimer = null;
      if (this.ws !== ws || this.closedByUser) return;
      console.warn("city client: connection attempt timed out, retrying");
      this.ws = null;
      this.scheduleReconnect();
      // Does not await events: the stalled socket is discarded, never used.
      ws.onopen = ws.onclose = ws.onmessage = ws.onerror = null;
      ws.close();
    }, CONNECT_TIMEOUT_MS);
    ws.onopen = () => {
      if (this.connectTimer !== null) {
        clearTimeout(this.connectTimer);
        this.connectTimer = null;
      }
      this.backoffAttempt = 0;
      this.setStatus("online");
      // identify must be the first message on every fresh connection; the
      // same key is reused so the server restores actor identity and label.
      this.transmit({ type: "identify", key: this.key });
    };
    ws.onmessage = (ev) => this.dispatch(ev.data);
    ws.onclose = () => {
      if (this.connectTimer !== null) {
        clearTimeout(this.connectTimer);
        this.connectTimer = null;
      }
      this.ws = null;
      this.failPending(new Error("connection lost before the server replied"));
      if (this.closedByUser) return;
      this.setStatus("offline");
      this.scheduleReconnect();
    };
    // onclose always follows onerror; nothing extra to do here.
    ws.onerror = () => {};
  }

  private scheduleReconnect(): void {
    const delay = Math.min(BASE_BACKOFF_MS * 2 ** this.backoffAttempt, MAX_BACKOFF_MS);
    this.backoffAttempt += 1;
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.closedByUser) this.open();
    }, delay);
  }

  private dispatch(raw: unknown): void {
    let message: ServerMessageTS;
    try {
      message = JSON.parse(String(raw)) as ServerMessageTS;
    } catch {
      console.warn("city client: undecodable message", raw);
      return;
    }
    switch (message.type) {
      case "hello":
        this.handlers.onIdentified(message.you);
        return;
      case "helloworld":
        this.handlers.onHello(message.world);
        return;
      case "event":
        this.handlers.onEvent(message.event);
        return;
      case "command_result": {
        const waiter = this.pending.get(message.result.command_id);
        if (!waiter) {
          console.warn("city client: result for unknown command", message.result);
          return;
        }
        this.pending.delete(message.result.command_id);
        waiter.resolve(message.result);
        return;
      }
      default:
        console.warn("city client: unknown message shape", message);
    }
  }

  private failPending(reason: Error): void {
    for (const waiter of this.pending.values()) waiter.reject(reason);
    this.pending.clear();
  }

  private setStatus(status: ConnectionStatus): void {
    if (this.currentStatus === status) return;
    this.currentStatus = status;
    this.handlers.onStatus(status);
  }
}
