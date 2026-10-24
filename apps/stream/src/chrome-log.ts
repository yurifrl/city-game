// Chrome stderr policy, inherited from the PoC bridge: headless Chrome spews a
// steady stream of GPU/dbus/fontconfig warnings that must not spam the service
// log. Interesting lines (DevTools endpoint, FATALs, crashes) are surfaced;
// everything else is only kept in the seat's bounded raw ring buffer for
// post-mortem (e.g. dumped when a seat fails to boot).

/** Ring buffer size for the unfiltered stderr history per seat. */
export const CHROME_RAW_LOG_LINES = 200;

/**
 * True when a chrome stderr line is worth printing to the service log.
 * The DevTools line is load-bearing (seat bootstrap parses it too).
 */
export function isInterestingChromeLine(line: string): boolean {
  if (line.trim() === '') return false;
  return /\bDevTools listening\b|\bFATAL\b|\bCheck failed\b|\bReceived signal\b|\bAborted\b|\bTrapped\b/.test(
    line,
  );
}
