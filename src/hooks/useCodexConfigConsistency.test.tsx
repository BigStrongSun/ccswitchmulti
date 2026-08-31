import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { CodexConfigConsistencyReport } from "@/lib/api/codexConfigConsistency";
import { codexConfigConsistencyApi } from "@/lib/api/codexConfigConsistency";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useCodexConfigConsistency } from "./useCodexConfigConsistency";

vi.mock("@/hooks/useTauriEvent", () => ({ useTauriEvent: vi.fn() }));
vi.mock("@/lib/api/codexConfigConsistency", () => ({
  codexConfigConsistencyApi: {
    inspect: vi.fn(),
    resolve: vi.fn(),
    inspectRuntimeRefresh: vi.fn(),
    refreshRuntimeState: vi.fn(),
  },
}));

const drift: CodexConfigConsistencyReport = {
  state: "external_drift",
  providerId: "router",
  expectedFingerprint: "expected",
  actualFingerprint: "actual",
  changedKeys: ["model_reasoning_effort"],
  reason: "live_config_changed",
  runtimeActivation: {
    state: "current",
    appServerStartedAt: "2026-09-01T00:00:00+08:00",
    configModifiedAt: "2026-08-31T23:59:00+08:00",
    reason: null,
  },
};

describe("useCodexConfigConsistency", () => {
  it("deduplicates the startup event and fallback query by actual fingerprint", async () => {
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(drift);
    let onEvent: ((payload: CodexConfigConsistencyReport) => void) | undefined;
    vi.mocked(useTauriEvent).mockImplementation((name, handler) => {
      if (name === "codex-config-consistency") {
        onEvent = handler as (payload: CodexConfigConsistencyReport) => void;
      }
    });

    const { result } = renderHook(() => useCodexConfigConsistency());
    await waitFor(() => expect(result.current.report).toEqual(drift));
    onEvent?.(drift);

    expect(result.current.report).toEqual(drift);
    expect(codexConfigConsistencyApi.inspect).toHaveBeenCalledOnce();
  });

  it("surfaces a runtime-only restart requirement even when disk config is consistent", async () => {
    const runtimeStale: CodexConfigConsistencyReport = {
      ...drift,
      state: "consistent",
      changedKeys: [],
      reason: null,
      runtimeActivation: {
        state: "restart_required",
        appServerStartedAt: "2026-08-31T21:48:34+08:00",
        configModifiedAt: "2026-08-31T22:38:53+08:00",
        reason: "app_server_started_before_managed_config",
      },
    };
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(
      runtimeStale,
    );

    const { result } = renderHook(() => useCodexConfigConsistency());

    await waitFor(() => expect(result.current.report).toEqual(runtimeStale));
  });

  it("requires a checked preflight before refreshing and preserves progress until completion", async () => {
    const runtimeStale: CodexConfigConsistencyReport = {
      ...drift,
      state: "consistent",
      changedKeys: [],
      reason: null,
      runtimeActivation: {
        state: "restart_required",
        appServerStartedAt: "2026-08-31T21:48:34+08:00",
        configModifiedAt: "2026-08-31T22:38:53+08:00",
        reason: "app_server_started_before_managed_config",
      },
    };
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(
      runtimeStale,
    );
    vi.mocked(
      codexConfigConsistencyApi.inspectRuntimeRefresh,
    ).mockResolvedValue({
      supported: true,
      canRefresh: true,
      snapshotToken: "checked-snapshot",
      desktopProcessCount: 1,
      appServerProcessCount: 1,
      processCount: 2,
      launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
      warning: "active_tasks_will_be_interrupted",
    });
    vi.mocked(codexConfigConsistencyApi.refreshRuntimeState).mockResolvedValue({
      forceTerminated: false,
      closedProcessCount: 2,
    });
    const { result } = renderHook(() => useCodexConfigConsistency());
    await waitFor(() => expect(result.current.report).toEqual(runtimeStale));

    await act(async () => result.current.inspectRuntimeRefresh());
    expect(result.current.refresh.phase).toBe("confirm");

    await act(async () => result.current.confirmRuntimeRefresh());
    expect(codexConfigConsistencyApi.refreshRuntimeState).toHaveBeenCalledWith(
      "checked-snapshot",
    );
    expect(result.current.refresh.phase).toBe("completed");
  });
});
