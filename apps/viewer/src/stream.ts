// One viewer connection: sends raw input, reports frames, captions and state.
// Reconnects with a bounded backoff; the state is always visible to the UI.

import { parseFrame, parseServerText, type Frame, type ViewerInput } from "./protocol";

export type ConnectionState = "connecting" | "live" | "reconnecting";

export interface StreamHandlers {
  onFrame(frame: Frame): void;
  onCaption(text: string): void;
  onState(state: ConnectionState, detail?: string): void;
}

const MAX_BACKOFF_MS = 5000;

export class Stream {
  private socket: WebSocket | undefined;
  private stopped = false;
  private attempt = 0;

  constructor(
    private readonly url: () => Promise<string>,
    private readonly handlers: StreamHandlers,
  ) {
    void this.connect();
  }

  send(input: ViewerInput): void {
    if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify(input));
  }

  close(): void {
    this.stopped = true;
    this.socket?.close();
  }

  private async connect(): Promise<void> {
    this.handlers.onState(this.attempt === 0 ? "connecting" : "reconnecting");
    let socket: WebSocket;
    try {
      socket = new WebSocket(await this.url());
    } catch (error) {
      this.retry(error instanceof Error ? error.message : String(error));
      return;
    }
    socket.binaryType = "arraybuffer";
    this.socket = socket;
    socket.onopen = () => {
      this.attempt = 0;
      this.handlers.onState("live");
    };
    socket.onmessage = (event) => {
      try {
        if (typeof event.data === "string") {
          const message = parseServerText(event.data);
          if (message.type === "caption") this.handlers.onCaption(message.text);
          else this.handlers.onState("reconnecting", message.message);
        } else {
          this.handlers.onFrame(parseFrame(event.data as ArrayBuffer));
        }
      } catch (error) {
        this.handlers.onState("reconnecting", `bad message from server: ${String(error)}`);
        socket.close();
      }
    };
    socket.onclose = () => this.retry();
  }

  private retry(detail?: string): void {
    if (this.stopped) return;
    this.attempt += 1;
    this.handlers.onState("reconnecting", detail);
    setTimeout(() => void this.connect(), Math.min(250 * 2 ** this.attempt, MAX_BACKOFF_MS));
  }
}
