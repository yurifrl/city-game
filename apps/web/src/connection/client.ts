// WebSocket transport for the command path. Connects to /ws (dev proxy targets
// the server), reconnects with 1s→10s exponential backoff reset on successful
// open, tracks pending commands by command_id, and dispatches server messages
// to the handlers wired by the composition root.

import type {
  CommandResultTS,
  CommandTS,
  ClientMessageTS,
  EventTS,
  ServerMessageTS,
  WorldTS,
} from "../protocol/types";

export type ConnectionStatus = "connecting" | "online" | "offline";

export interface ClientHandlers {
  onHello(world: WorldTS, actor: string): void;
  onEvent(event: EventTS): void;
  onStatus(status: ConnectionStatus): void;
}

const BASE_BACKOFF_MS = 1000;
const MAX_BACKOFF_MS = 10000;

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
  private backoffAttempt = 0;
  private closedByUser = false;
  private currentStatus: ConnectionStatus = "offline";

  constructor(
    private readonly handlers: ClientHandlers,
    private readonly url: string = defaultUrl(),
  ) {}

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
      const message: ClientMessageTS = { type: "command", command };
      ws.send(JSON.stringify(message));
    });
  }

  close(): void {
    this.closedByUser = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.failPending(new Error("client closed before the server replied"));
    this.ws?.close();
    this.ws = null;
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
    ws.onopen = () => {
      this.backoffAttempt = 0;
      this.setStatus("online");
    };
    ws.onmessage = (ev) => this.dispatch(ev.data);
    ws.onclose = () => {
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
        this.handlers.onHello(message.world, message.actor);
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
