// Warm seat pool: boot/replace seats, attach/detach viewers, first-viewer
// input control, status snapshot, and a janitor that reaps dead seats and
// tops the warm pool back up.

import { newSeatId, sleep, waitFor } from './ids';
import { Seat, type Frame } from './seat';
import type { StreamEnv } from './env';
import type { SeatStatus, StatusResponse, ViewerInput } from './types';

/** Hard cap on total live seats (warm pool + dynamic POST /api/seat). */
const MAX_SEATS = 64;

const JANITOR_INTERVAL_MS = 5_000;

export interface ViewerAttach {
  seatId: string;
  viewerId: string;
  role: 'controller' | 'spectator';
}

export class PoolError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

export class SeatPool {
  readonly env: StreamEnv;
  /** Upstream game origin (also used by the /api/seed proxy). */
  get gameUrl(): string {
    return this.env.gameUrl;
  }

  private readonly seats = new Map<string, Seat>();
  /** seatId → attached viewer ids in attachment order (first = controller). */
  private readonly viewers = new Map<string, Set<string>>();
  private janitor?: ReturnType<typeof setInterval>;
  private starting = false;

  constructor(env: StreamEnv) {
    this.env = env;
  }

  /** Seat viewport advertised to viewers in the hello. */
  viewport(): { width: number; height: number; dpr: number } {
    return { width: this.env.width, height: this.env.height, dpr: this.env.dpr };
  }

  /** Boot the warm pool. Safe to call twice. */
  async start(): Promise<void> {
    if (this.janitor !== undefined) return;
    this.janitor = setInterval(() => {
      void this.janitorPass();
    }, this.env.janitorMs);
    this.janitor.unref?.();
    await this.topUp();
  }

  /** Stop every seat and the janitor. Idempotent. */
  async stop(): Promise<void> {
    if (this.janitor !== undefined) {
      clearInterval(this.janitor);
      this.janitor = undefined;
    }
    const seats = [...this.seats.values()];
    this.seats.clear();
    this.viewers.clear();
    await Promise.allSettled(seats.map((seat) => seat.stop()));
  }

  /** Block until every warm seat is ready (e2e gate). */
  async waitReady(timeoutMs?: number): Promise<void> {
    const expected = this.env.seats;
    await waitFor(() => {
      const usable = [...this.seats.values()].filter((seat) => {
        const state = this.wireState(seat);
        return state === 'idle' || state === 'ready' || state === 'busy';
      });
      return expected === 0 || usable.length >= expected;
    }, timeoutMs ?? this.env.bootTimeoutMs + 5_000, 'pool readiness');
  }

  /** Boot one extra seat (POST /api/seat). Resolves when it shows frames. */
  async createSeat(id?: string): Promise<Seat> {
    if (this.seats.size >= MAX_SEATS) throw new PoolError(`seat limit reached (${MAX_SEATS})`, 429);
    const seat = new Seat(id ?? newSeatId(), this.env);
    this.seats.set(seat.id, seat);
    this.viewers.set(seat.id, new Set());
    try {
      await seat.boot();
    } catch (err) {
      this.seats.delete(seat.id);
      this.viewers.delete(seat.id);
      throw err;
    }
    return seat;
  }

  /** Stop and drop a seat (DELETE /api/seat). Throws when unknown. */
  async destroySeat(id: string): Promise<void> {
    const seat = this.seats.get(id);
    if (seat === undefined) throw new PoolError(`unknown seat: ${id}`, 404);
    this.seats.delete(id);
    this.viewers.delete(id);
    await seat.stop();
  }

  get(seatId: string): Seat | undefined {
    return this.seats.get(seatId);
  }

  list(): Seat[] {
    return [...this.seats.values()];
  }

  /** Least-loaded usable seat id, or undefined (used for GET /view). */
  bestSeatId(): string | undefined {
    return this.pickSeat()?.id;
  }

  /**
   * Attach a viewer. Prefers the given seat, else the least-loaded ready
   * seat, else any live seat (viewer waits for frames). First viewer on a
   * seat becomes its controller.
   */
  attachViewer(preferredSeatId?: string): ViewerAttach {
    const seat = this.pickSeat(preferredSeatId);
    if (seat === undefined) throw new PoolError('no live seats', 503);
    const set = this.viewers.get(seat.id);
    if (set === undefined) throw new PoolError(`seat vanished: ${seat.id}`, 503);
    const viewerId = crypto.randomUUID();
    set.add(viewerId);
    return { seatId: seat.id, viewerId, role: [...set][0] === viewerId ? 'controller' : 'spectator' };
  }

  /** Detach; hands controller to the earliest remaining viewer. */
  detachViewer(seatId: string, viewerId: string): void {
    this.viewers.get(seatId)?.delete(viewerId);
  }

  /** Current controller of a seat, if any. */
  holder(seatId: string): string | null {
    return [...(this.viewers.get(seatId) ?? [])][0] ?? null;
  }

  viewerCount(seatId: string): number {
    return this.viewers.get(seatId)?.size ?? 0;
  }

  /** Validate + forward a viewer input; only the controller may inject. */
  async dispatchInput(seatId: string, viewerId: string, input: ViewerInput): Promise<void> {
    if (this.holder(seatId) !== viewerId) {
      throw new PoolError('only the first viewer of a seat may inject input', 409);
    }
    const seat = this.seats.get(seatId);
    if (seat === undefined) throw new PoolError(`unknown seat: ${seatId}`, 404);
    await seat.dispatchInput(input);
  }

  status(): StatusResponse {
    const seats: SeatStatus[] = this.list().map((seat) => {
      const rt = seat.runtime();
      const entry: SeatStatus = { id: rt.id, state: this.wireState(seat), viewers: this.viewerCount(rt.id) };
      if (rt.pid !== undefined) entry.pid = rt.pid;
      if (seat.ready) entry.size = { width: this.env.width, height: this.env.height, dpr: this.env.dpr };
      if (rt.debugPort !== undefined) entry.debugPort = rt.debugPort;
      if (rt.error !== undefined) entry.lastError = rt.error;
      return entry;
    });
    return { seats };
  }

  private wireState(seat: Seat): SeatStatus['state'] {
    if (seat.status === 'error') return 'error';
    if (seat.status !== 'ready') return 'booting';
    const viewers = this.viewerCount(seat.id);
    if (viewers === 0) return 'idle';
    return viewers > 1 ? 'busy' : 'ready';
  }

  private pickSeat(preferredSeatId?: string): Seat | undefined {
    if (preferredSeatId !== undefined) {
      const seat = this.seats.get(preferredSeatId);
      if (seat !== undefined) return seat;
    }
    const live = this.list();
    const ready = live.filter((s) => s.ready);
    const candidates = ready.length > 0 ? ready : live;
    return candidates.reduce<Seat | undefined>(
      (best, s) => (best === undefined || this.viewerCount(s.id) < this.viewerCount(best.id) ? s : best),
      undefined,
    );
  }

  /** Reap dead seats, top the warm pool back up, bounded concurrency. */
  private async janitorPass(): Promise<void> {
    for (const [id, seat] of this.seats) {
      if (seat.status === 'error' || seat.status === 'stopped') {
        this.seats.delete(id);
        this.viewers.delete(id);
        await seat.stop().catch(() => {});
      }
    }
    await this.topUp();
  }

  private async topUp(): Promise<void> {
    if (this.starting) return;
    const missing = this.env.seats - [...this.seats.values()].filter((s) => s.status === 'ready' || s.status === 'booting').length;
    if (missing <= 0) return;
    this.starting = true;
    try {
      await Promise.allSettled(Array.from({ length: missing }, () => this.createSeat()));
    } finally {
      this.starting = false;
    }
  }
}

/** Test helper: bounded wait for a pool predicate. */
export async function poolEventually(pool: SeatPool, fn: () => boolean, timeoutMs: number, label: string): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (fn()) return;
    if (Date.now() > deadline) throw new Error(`${label}: condition not met within ${timeoutMs}ms`);
    await sleep(25);
  }
}

export type { Frame };
