/** A push source read as an async iterable — the adapter between a callback feed
 *  and a `for await` loop.
 *
 *  komo's turn reports progress by callback (`runTurn`'s `onTools`), but
 *  assistant-ui's streaming adapter is a generator that must *pull*. This is the
 *  join: the producer calls `push`, the consumer iterates.
 *
 *  Two properties make it correct for that job. It **coalesces**: only the
 *  latest value is held, so a consumer slower than a burst of tool frames skips
 *  to the current state instead of replaying every intermediate one (each push
 *  is a whole snapshot, so a skipped one is genuinely redundant). And abandoning
 *  the iterator early — which is what an aborted turn does — is safe: `push`
 *  after `close`, and `close` with nobody waiting, are both no-ops. */
export function pushStream<T>() {
  let latest: { value: T } | undefined;
  let closed = false;
  let wake: (() => void) | undefined;

  return {
    push(value: T): void {
      if (closed) return;
      latest = { value };
      wake?.();
    },
    close(): void {
      closed = true;
      wake?.();
    },
    async *[Symbol.asyncIterator](): AsyncGenerator<T, void> {
      for (;;) {
        if (latest) {
          const { value } = latest;
          latest = undefined;
          yield value;
          continue;
        }
        // Nothing pending: a close now means the stream is genuinely done,
        // rather than momentarily empty.
        if (closed) return;
        await new Promise<void>((resolve) => {
          wake = resolve;
        });
        wake = undefined;
      }
    },
  };
}

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
