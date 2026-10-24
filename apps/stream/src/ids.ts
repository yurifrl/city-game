// Small shared helpers for the stream service.

/** Fresh seat id, URL-safe and short enough to read aloud. */
export function newSeatId(): string {
  return `seat-${crypto.randomUUID().slice(0, 8)}`;
}

export function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

export function sleep(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}

/** Bounded wait for a condition; rejects on timeout with `label`. */
export async function waitFor<T>(
  fn: () => T | Promise<T>,
  timeoutMs: number,
  label: string,
  intervalMs = 100,
): Promise<NonNullable<Awaited<T>>> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`${label}: condition not met within ${timeoutMs}ms`);
    await sleep(intervalMs);
  }
}
