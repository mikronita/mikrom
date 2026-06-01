export function consumeSseBuffer(buffer: string, onMessage: (payload: unknown) => void) {
  const normalizedBuffer = buffer.replace(/\r\n/g, "\n");
  const parts = normalizedBuffer.split("\n\n");
  const remainder = normalizedBuffer.endsWith("\n\n") ? "" : parts.pop() || "";

  for (const rawEvent of parts) {
    if (!rawEvent.trim()) continue;

    const dataLines: string[] = [];
    for (const line of rawEvent.split("\n")) {
      if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).replace(/^ /, ""));
      }
    }

    if (dataLines.length === 0) continue;

    let parsed: unknown;
    try {
      parsed = JSON.parse(dataLines.join("\n"));
    } catch {
      // Ignore malformed events.
      continue;
    }

    onMessage(parsed);
  }

  return remainder;
}

export function createFetchSseStream(
  url: string,
  init: RequestInit,
  onMessage: (payload: unknown) => void,
  options: { retryDelayMs?: number } = {},
) {
  const controller = new AbortController();
  const retryDelayMs = options.retryDelayMs ?? 1000;

  const wait = () =>
    new Promise<void>((resolve) => {
      const timer = setTimeout(() => resolve(), retryDelayMs);
      controller.signal.addEventListener(
        "abort",
        () => {
          clearTimeout(timer);
          resolve();
        },
        { once: true },
      );
    });

  void (async () => {
    while (!controller.signal.aborted) {
      const response = await fetch(url, {
        ...init,
        signal: controller.signal,
      }).catch(() => null);

      if (!response) {
        if (!controller.signal.aborted) {
          await wait();
        }
        continue;
      }

      if (response.status === 401 || response.status === 403) {
        break;
      }

      if (!response.ok || !response.body) {
        await wait();
        continue;
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

      while (!controller.signal.aborted) {
        let result: ReadableStreamReadResult<Uint8Array> | null;
        try {
          result = await reader.read();
        } catch {
          result = null;
        }

        if (!result) {
          if (!controller.signal.aborted) {
            await wait();
          }
          break;
        }

        const { value, done } = result;
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        buffer = consumeSseBuffer(buffer, onMessage);
      }

      buffer += decoder.decode();
      consumeSseBuffer(buffer, onMessage);
    }
  })();

  return () => controller.abort();
}
