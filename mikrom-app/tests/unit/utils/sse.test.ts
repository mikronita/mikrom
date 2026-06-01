import { describe, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/svelte";
import { createFetchSseStream, consumeSseBuffer } from "$lib/utils/sse";

describe("sse utils", () => {
  it("parses chunked SSE payloads and preserves trailing partial data", () => {
    const onMessage = vi.fn();

    const remainder = consumeSseBuffer('data: {"kind":"refresh"', onMessage);

    expect(remainder).toBe('data: {"kind":"refresh"');
    expect(onMessage).not.toHaveBeenCalled();
  });

  it("parses completed SSE events from buffered chunks", () => {
    const onMessage = vi.fn();
    const buffer = 'data: {"kind":"refresh"}\n\ndata: {"kind":"deployment_changed"}\n\n';

    const remainder = consumeSseBuffer(buffer, onMessage);

    expect(remainder).toBe("");
    expect(onMessage).toHaveBeenCalledTimes(2);
    expect(onMessage).toHaveBeenNthCalledWith(1, { kind: "refresh" });
    expect(onMessage).toHaveBeenCalledWith({ kind: "deployment_changed" });
  });

  it("does not retry on unauthorized SSE responses", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      body: null,
    });

    vi.stubGlobal("fetch", fetchMock);

    const cleanup = createFetchSseStream("/api/v1/workspace/events", {}, vi.fn());

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(fetchMock).toHaveBeenCalledTimes(1);

    cleanup();
  });
});
