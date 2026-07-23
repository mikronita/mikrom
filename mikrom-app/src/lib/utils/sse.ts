import { logout } from "$lib/auth";

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
  options: {
    retryDelayMs?: number;
    maxRetryDelayMs?: number;
    onUnauthorized?: () => void;
  } = {},
) {
  const controller = new AbortController();
  const baseRetryDelayMs = options.retryDelayMs ?? 1000;
  const maxRetryDelayMs = options.maxRetryDelayMs ?? 30000;
  const handleUnauthorized = options.onUnauthorized ?? logout;

  let attemptCount = 0;

  const wait = () => {
    attemptCount += 1;
    const exponentialDelay = baseRetryDelayMs * Math.pow(2, attemptCount - 1);
    const delay = Math.min(maxRetryDelayMs, exponentialDelay);

    return new Promise<void>((resolve) => {
      const timer = setTimeout(() => resolve(), delay);
      controller.signal.addEventListener(
        "abort",
        () => {
          clearTimeout(timer);
          resolve();
        },
        { once: true },
      );
    });
  };

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
        handleUnauthorized();
        break;
      }

      if (!response.ok || !response.body) {
        await wait();
        continue;
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

      // Successfully connected to response body
      let receivedAnyData = false;

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

        if (!receivedAnyData) {
          receivedAnyData = true;
          attemptCount = 0;
        }

        buffer += decoder.decode(value, { stream: true });
        buffer = consumeSseBuffer(buffer, onMessage);
      }

      buffer += decoder.decode();
      consumeSseBuffer(buffer, onMessage);
    }
  })();

  return () => controller.abort();
}
