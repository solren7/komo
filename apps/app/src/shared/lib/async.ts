/** `setTimeout` as a promise that resolves early when `signal` aborts — so a
 *  finished turn never leaves a poll loop idling out its last interval. */
export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) return Promise.resolve();
  return new Promise((resolve) => {
    // Whichever path fires first disarms the other, so `done` runs once.
    const done = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", done);
      // oxlint-disable-next-line promise/no-multiple-resolved
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal?.addEventListener("abort", done, { once: true });
  });
}
