import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { CodexConfigConsistencyReport } from "@/lib/api/codexConfigConsistency";
import { codexConfigConsistencyApi } from "@/lib/api/codexConfigConsistency";
import { proxyApi } from "@/lib/api/proxy";
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
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    unlockCodexModelPicker: vi.fn(),
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

  it("proactively opens the repair status when Codex history projection damage is detected", async () => {
    const consistent: CodexConfigConsistencyReport = {
      ...drift,
      state: "consistent",
      changedKeys: [],
      reason: null,
    };
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(consistent);
    vi.mocked(
      codexConfigConsistencyApi.inspectRuntimeRefresh,
    ).mockResolvedValue({
      supported: true,
      canRefresh: true,
      snapshotToken: "history-damage-snapshot",
      desktopProcessCount: 1,
      appServerProcessCount: 1,
      processCount: 2,
      launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
      warning: null,
      paginatedHistory: {
        affectedRolloutCount: 1,
        duplicateOrdinalCount: 0,
        rotatedThreadCount: 1,
        rotatedSegmentCount: 3,
        affectedBytes: 1_400_000_000,
        blockedRolloutCount: 0,
        blockedReason: null,
      },
    });

    const { result } = renderHook(() => useCodexConfigConsistency());

    await waitFor(() => expect(result.current.refresh.phase).toBe("status"));
    expect(result.current.refresh.preflight?.paginatedHistory).toMatchObject({
      rotatedThreadCount: 1,
      rotatedSegmentCount: 3,
    });
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
      paginatedHistory: {
        affectedRolloutCount: 1,
        duplicateOrdinalCount: 3,
        affectedBytes: 1_100_000_000,
        blockedRolloutCount: 0,
        blockedReason: null,
      },
    });
    vi.mocked(codexConfigConsistencyApi.refreshRuntimeState).mockResolvedValue({
      outcome: "completed_with_warnings",
      configStatus: "ready",
      paginatedHistoryStatus: "ready",
      rendererCompatibilityStatus: "warning",
      rendererCompatibilityMessage: "old renderer path",
      forceTerminated: false,
      closedProcessCount: 2,
      repairedHistoryRolloutCount: 1,
      repairedHistoryDuplicateCount: 3,
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

    vi.mocked(proxyApi.unlockCodexModelPicker).mockResolvedValue({
      attemptedPorts: [9229],
      debugPort: 9229,
      targetId: "renderer",
      targetTitle: "ChatGPT",
      targetUrl: "app://-/index.html",
      modelCount: 1,
      modelNames: ["qwen3.8"],
      injected: true,
      launched: false,
      codexExecutable: "ChatGPT.exe",
      historySyncRequested: true,
      historyCatalogComplete: true,
      historyCatalogCount: 100,
      allProviderHistoryPatched: true,
      historyRefreshRequested: true,
      message: "ready",
    });
    await act(async () => result.current.retryRendererCompatibility());
    expect(result.current.refresh.result?.outcome).toBe("completed");
    expect(result.current.refresh.result?.rendererCompatibilityStatus).toBe(
      "ready",
    );
  });

  it("opens a persistent status overview even when config is already consistent", async () => {
    const consistent: CodexConfigConsistencyReport = {
      ...drift,
      state: "consistent",
      changedKeys: [],
      reason: null,
    };
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(consistent);
    vi.mocked(
      codexConfigConsistencyApi.inspectRuntimeRefresh,
    ).mockResolvedValue({
      supported: true,
      canRefresh: true,
      snapshotToken: "status-snapshot",
      desktopProcessCount: 1,
      appServerProcessCount: 1,
      processCount: 2,
      launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
      warning: null,
      paginatedHistory: {
        affectedRolloutCount: 0,
        duplicateOrdinalCount: 0,
        affectedBytes: 0,
        blockedRolloutCount: 0,
        blockedReason: null,
      },
    });
    const { result } = renderHook(() => useCodexConfigConsistency());
    await waitFor(() =>
      expect(codexConfigConsistencyApi.inspect).toHaveBeenCalled(),
    );

    await act(async () => result.current.openStatusPanel());

    expect(result.current.report).toEqual(consistent);
    expect(result.current.refresh.phase).toBe("status");
    expect(result.current.refresh.preflight?.snapshotToken).toBe(
      "status-snapshot",
    );
  });
});
