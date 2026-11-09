// Retry cancellation until native admission is acknowledged or the worker ends.
// Only one cancellation request is in flight; completion still awaits the worker.
export async function runNativeOperation<T>(
  work: () => Promise<unknown>,
  decode: (value: unknown) => T,
  cancelNative: () => Promise<boolean>,
  signal?: AbortSignal,
): Promise<T> {
  signal?.throwIfAborted();
  let pending = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancellation: Promise<void> | undefined;
  const cancel = () => {
    if (!pending) return;
    cancellation = (async () => {
      let acknowledged = false;
      try {
        acknowledged = await cancelNative();
      } catch {
        // Retry transport failure too; a completed worker ends retries below.
      }
      if (pending && !acknowledged) timer = setTimeout(cancel, 100);
    })();
  };
  signal?.addEventListener('abort', cancel, { once: true });
  try {
    const value = await work();
    signal?.throwIfAborted();
    return decode(value);
  } catch (error) {
    signal?.throwIfAborted();
    throw error;
  } finally {
    pending = false;
    clearTimeout(timer);
    signal?.removeEventListener('abort', cancel);
    await cancellation;
  }
}
