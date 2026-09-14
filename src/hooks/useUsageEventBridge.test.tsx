import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUsageEventBridge } from "./useUsageEventBridge";

type EventHandler = (event: { payload: unknown }) => void;

const listeners = vi.hoisted(() => new Map<string, EventHandler>());
const unlisten = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() =>
  vi.fn(async (event: string, handler: EventHandler) => {
    listeners.set(event, handler);
    return unlisten;
  }),
);

vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
}));

function Bridge() {
  useUsageEventBridge();
  return null;
}

function renderBridge(queryClient = new QueryClient()) {
  return render(
    <QueryClientProvider client={queryClient}>
      <Bridge />
    </QueryClientProvider>,
  );
}

describe("useUsageEventBridge", () => {
  afterEach(() => {
    listeners.clear();
    unlisten.mockReset();
    listenMock.mockReset();
    listenMock.mockImplementation(
      async (event: string, handler: EventHandler) => {
        listeners.set(event, handler);
        return unlisten;
      },
    );
  });

  it("invalidates only collector status and session stats for a newer collection event", async () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    renderBridge(queryClient);

    await waitFor(() =>
      expect(listeners.get("session-collection-updated")).toBeTypeOf(
        "function",
      ),
    );
    listeners.get("session-collection-updated")?.({
      payload: { revision: 2, phase: "idle" },
    });

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["usage", "session-collection-status"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["usage", "codex-subagent-stats"],
    });
  });

  it("drops an out-of-order collector notification", async () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    renderBridge(queryClient);

    await waitFor(() =>
      expect(listeners.get("session-collection-updated")).toBeTypeOf(
        "function",
      ),
    );
    const listener = listeners.get("session-collection-updated")!;
    listener({ payload: { revision: 5, phase: "idle" } });
    listener({ payload: { revision: 4, phase: "running" } });

    expect(invalidateQueries).toHaveBeenCalledTimes(2);
  });

  it("unsubscribes an asynchronously resolved listener after unmount", async () => {
    let resolveCollection!: (value: typeof unlisten) => void;
    listenMock
      .mockImplementationOnce(async (event: string, handler: EventHandler) => {
        listeners.set(event, handler);
        return unlisten;
      })
      .mockImplementationOnce(
        () =>
          new Promise<typeof unlisten>((resolve) => {
            resolveCollection = resolve;
          }),
      );
    const view = renderBridge();
    await waitFor(() => expect(listenMock).toHaveBeenCalledTimes(2));
    view.unmount();
    resolveCollection(unlisten);
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(2));
  });

  it("does not invalidate after unmount even if a retained event callback fires", async () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const view = renderBridge(queryClient);
    await waitFor(() =>
      expect(listeners.get("session-collection-updated")).toBeTypeOf(
        "function",
      ),
    );
    const listener = listeners.get("session-collection-updated")!;
    view.unmount();
    listener({ payload: { revision: 99, phase: "idle" } });

    expect(invalidateQueries).not.toHaveBeenCalled();
  });
});
