// apps/stream — server-rendered video bridge (Bun + TypeScript).
//
// Run order (see README.md):
//   1. game server (crates/server) on CITY_GAME_URL (default :8080)
//   2. this service: bun run src/index.ts  (listens :8081, boots CITY_SEATS warm seats)
//   3. open http://127.0.0.1:8081/view/{seatId} (seatId from POST /api/seat)
//
// This file duplicates the small stream-facing wire types locally. The stream
// service never imports from apps/web or crates: downstream it forwards opaque
// JPEG frames, upstream it speaks its own tiny viewer protocol.

/** Public seat lifecycle. Computed by the pool from seat + viewer state. */
export type SeatState = 'booting' | 'idle' | 'ready' | 'busy' | 'error';

/** Seat page viewport in CSS px + device scale factor. */
export interface Viewport {
  width: number;
  height: number;
  dpr: number;
}

/** Per-seat entry of GET /api/status.json. Extra diagnostic keys are additive. */
export interface SeatStatus {
  id: string;
  state: SeatState;
  viewers: number;
  /** Chrome child pid (present once spawned). */
  pid?: number;
  /** Current viewport (present once booted). */
  size?: Viewport;
  /** DevTools port of the seat's Chrome child (debugging aid). */
  debugPort?: number;
  /** Set when the seat failed to boot / crashed. */
  lastError?: string;
}

export interface StatusResponse {
  seats: SeatStatus[];
}

/** Viewer → service, JSON text frame on /ws/{seatId}. Coordinates are seat-CSS-px. */
export interface ViewerInput {
  type: 'input';
  x: number;
  y: number;
  /** Pointer buttons bitmask, passed through to CDP. */
  buttons: number;
  down: boolean;
  /** Present only for wheel events (then deltaX/deltaY apply). */
  kind?: 'wheel';
  deltaX?: number;
  deltaY?: number;
}

/** Binary frame layout to viewers: u32be jsonLen | JSON(FrameMeta) | JPEG bytes. */
export interface FrameMeta {
  format: 'jpeg';
  seq: number;
  /** JPEG intrinsic (device) pixel size. */
  w: number;
  h: number;
  /** Seat CSS viewport the pointer coordinates should be expressed in. */
  cssWidth: number;
  cssHeight: number;
  dpr: number;
  ts: number;
}

/** Service → viewer, one JSON text message on ws open. */
export interface ViewerHello {
  type: 'hello';
  role: 'controller' | 'spectator';
  seatId: string;
  viewport: Viewport;
}

/** Service → viewer on seat failure, followed by ws close. */
export interface ViewerError {
  type: 'error';
  message: string;
}
