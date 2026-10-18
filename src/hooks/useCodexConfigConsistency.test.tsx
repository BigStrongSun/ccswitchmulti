import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";

import type {
  CodexConfigConsistencyReport,
  CodexRuntimeRefreshProgress,
  CodexRuntimeRefreshPreflight,
  CodexRuntimeRefreshResult,
} from "@/lib/api/codexConfigConsistency";
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
  it.each([
    ["recheck", false],
    ["recheck", true],
    ["resolve", false],
    ["resolve", true],
  ] as const)(
    "discards stale %s after native switch (reject=%s)",
    async (entry, reject) => {
      vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(drift);
      let onEvent:
        | ((payload: CodexConfigConsistencyReport) => void)
        | undefined;
      vi.mocked(useTauriEvent).mockImplementation((name, handler) => {
        if (name === "codex-config-consistency")
          onEvent = handler as typeof onEvent;
      });
      const { result } = renderHook(() => useCodexConfigConsistency());
      await waitFor(() => expect(result.current.report).toEqual(drift));
      let resolveRequest!: (value: CodexConfigConsistencyReport) => void;
      let rejectRequest!: (error: Error) => void;
      const request = new Promise<CodexConfigConsistencyReport>(
        (resolve, reject) => {
          resolveRequest = resolve;
          rejectRequest = reject;
        },
      );
      if (entry === "recheck")
        vi.mocked(codexConfigConsistencyApi.inspect).mockReturnValue(request);
      else
        vi.mocked(codexConfigConsistencyApi.resolve).mockReturnValue(request);
      let pending!: Promise<void>;
      await act(async () => {
        pending =
          entry === "recheck"
            ? result.current.recheck()
            : result.current.resolve("apply_ccsm");
      });
      act(() =>
        onEvent?.({
          ...drift,
          state: "not_applicable",
          reason: "native_codex_config",
        }),
      );
      const pendingAfterNative = result.current.pending;
      await act(async () => {
        if (reject) rejectRequest(new Error("stale managed request failed"));
        else resolveRequest(drift);
        await pending;
      });
      expect(pendingAfterNative).toBe(false);
      expect(result.current.report).toBeNull();
      expect(result.current.error).toBeNull();
      expect(result.current.pending).toBe(false);
      expect(result.current.refresh.phase).toBe("idle");
    },
  );

  it.each([
    ["automatic", false],
    ["automatic", true],
    ["confirm", false],
    ["confirm", true],
    ["status", false],
    ["status", true],
  ] as const)(
    "discards stale %s preflight after native switch (reject=%s)",
    async (entry, reject) => {
      const consistent = {
        ...drift,
        state: "consistent" as const,
        reason: null,
      };
      vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue(
        entry === "automatic" ? consistent : drift,
      );
      let resolvePreflight!: (value: CodexRuntimeRefreshPreflight) => void;
      let rejectPreflight!: (error: Error) => void;
      vi.mocked(
        codexConfigConsistencyApi.inspectRuntimeRefresh,
      ).mockReturnValue(
        new Promise((resolve, reject) => {
          resolvePreflight = resolve;
          rejectPreflight = reject;
        }),
      );
      let onEvent:
        | ((payload: CodexConfigConsistencyReport) => void)
        | undefined;
      vi.mocked(useTauriEvent).mockImplementation((name, handler) => {
        if (name === "codex-config-consistency")
          onEvent = handler as typeof onEvent;
      });
      const { result } = renderHook(() => useCodexConfigConsistency());
      let pending: Promise<void> | undefined;
      if (entry !== "automatic") {
        await waitFor(() => expect(result.current.report).toEqual(drift));
        await act(async () => {
          pending =
            entry === "confirm"
              ? result.current.inspectRuntimeRefresh()
              : result.current.openStatusPanel();
        });
      }
      await waitFor(() =>
        expect(
          codexConfigConsistencyApi.inspectRuntimeRefresh,
        ).toHaveBeenCalledOnce(),
      );
      act(() =>
        onEvent?.({
          ...consistent,
          state: "not_applicable",
          reason: "native_codex_config",
        }),
      );
      expect(result.current.refresh.phase).toBe("idle");
      await act(async () => {
        if (reject) rejectPreflight(new Error("stale inspection error"));
        else
          resolvePreflight({
            supported: true,
            canRefresh: true,
            snapshotToken: "stale-managed-snapshot",
            desktopProcessCount: 1,
            appServerProcessCount: 1,
            processCount: 2,
            launchTarget: "Codex.exe",
            warning: null,
            paginatedHistory: {
              affectedRolloutCount: 1,
              duplicateOrdinalCount: 1,
              affectedBytes: 100,
              blockedRolloutCount: 0,
              blockedReason: null,
            },
          });
        await pending;
      });
      expect(result.current.refresh.phase).toBe("idle");
      expect(result.current.refresh.preflight).toBeNull();
      expect(result.current.refresh.error).toBeNull();
      expect(result.current.report).toBeNull();
    },
  );

  it("does not inspect CCSM runtime or history damage for native Codex", async () => {
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue({
      ...drift,
      state: "not_applicable",
      reason: "native_codex_config",
      runtimeActivation: {
        state: "unknown",
        appServerStartedAt: null,
        configModifiedAt: null,
        reason: "native_codex_config",
      },
    });
    const { result } = renderHook(() => useCodexConfigConsistency());

    await act(async () => {});

    expect(codexConfigConsistencyApi.inspect).toHaveBeenCalledOnce();
    expect(
      codexConfigConsistencyApi.inspectRuntimeRefresh,
    ).not.toHaveBeenCalled();
    expect(result.current.refresh.phase).toBe("idle");
    expect(result.current.report).toBeNull();
  });

  it("shows localized native ownership without inspecting history when opening status", async () => {
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    vi.mocked(codexConfigConsistencyApi.inspect).mockResolvedValue({
      ...drift,
      state: "not_applicable",
      reason: "native_codex_config",
    });
    const { result } = renderHook(() => useCodexConfigConsistency());
    await act(async () => {});

    await act(async () => result.current.openStatusPanel());

    expect(
      codexConfigConsistencyApi.inspectRuntimeRefresh,
    ).not.toHaveBeenCalled();
    expect(result.current.refresh.error).toBe(
      zh.notifications.nativeCodexConfigOwned,
    );
    expect(
      codexConfigConsistencyApi.refreshRuntimeState,
    ).not.toHaveBeenCalled();
  });

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

  it("keeps the status panel closed for history files that are only kept unchanged", async () => {
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
      snapshotToken: "skipped-history-snapshot",
      desktopProcessCount: 1,
      appServerProcessCount: 1,
      processCount: 2,
      launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
      warning: null,
      paginatedHistory: {
        affectedRolloutCount: 0,
        duplicateOrdinalCount: 0,
        affectedBytes: 0,
        blockedRolloutCount: 38,
        blockedReason:
          "history_base_offset_not_record_boundary: rollout_id=01a00000-0000-7000-8000-000000000002",
      },
    });

    const { result } = renderHook(() => useCodexConfigConsistency());

    await waitFor(() =>
      expect(
        codexConfigConsistencyApi.inspectRuntimeRefresh,
      ).toHaveBeenCalled(),
    );
    expect(result.current.refresh.phase).toBe("idle");
    expect(result.current.report).toBeNull();
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

  it("captures detailed progress logs and keeps heartbeats out of the log list", async () => {
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
      snapshotToken: "progress-snapshot",
      desktopProcessCount: 1,
      appServerProcessCount: 1,
      processCount: 2,
      launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
      warning: null,
      paginatedHistory: {
        affectedRolloutCount: 1,
        duplicateOrdinalCount: 3,
        affectedBytes: 1_100_000_000,
        blockedRolloutCount: 0,
        blockedReason: null,
      },
    });
    vi.mocked(codexConfigConsistencyApi.refreshRuntimeState).mockReturnValue(
      new Promise(() => {}),
    );
    let onProgress:
      | ((payload: CodexRuntimeRefreshProgress) => void)
      | undefined;
    vi.mocked(useTauriEvent).mockImplementation((name, handler) => {
      if (name === "codex-runtime-refresh-progress") {
        onProgress = handler as (payload: CodexRuntimeRefreshProgress) => void;
      }
    });

    const { result } = renderHook(() => useCodexConfigConsistency());
    await waitFor(() => expect(result.current.report).toEqual(runtimeStale));
    await act(async () => result.current.inspectRuntimeRefresh());
    await act(async () => {
      void result.current.confirmRuntimeRefresh();
    });
    expect(result.current.refresh.phase).toBe("refreshing");

    act(() => {
      onProgress?.({
        stage: "repairing_history",
        kind: "log",
        sequence: 1,
        emittedAtMs: 1_700_000_000_000,
        code: "history_file_started",
        message: "正在修复历史文件 1/2：abc",
      });
      onProgress?.({
        stage: "repairing_history",
        kind: "heartbeat",
        sequence: 2,
        emittedAtMs: 1_700_000_000_100,
      });
    });

    expect(result.current.refresh.logs).toHaveLength(1);
    expect(result.current.refresh.logs[0]?.message).toBe(
      "正在修复历史文件 1/2：abc",
    );
    expect(result.current.refresh.lastProgressAt).not.toBeNull();
  });

  it("fails a silent refresh and ignores the late backend result", async () => {
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
      snapshotToken: "stall-snapshot",
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
    let resolveRefresh!: (value: CodexRuntimeRefreshResult) => void;
    vi.mocked(codexConfigConsistencyApi.refreshRuntimeState).mockReturnValue(
      new Promise((resolve) => {
        resolveRefresh = resolve;
      }),
    );

    const { result } = renderHook(() => useCodexConfigConsistency());
    await waitFor(() => expect(result.current.report).toEqual(runtimeStale));
    await act(async () => result.current.inspectRuntimeRefresh());
    expect(result.current.refresh.phase).toBe("confirm");

    vi.useFakeTimers();
    try {
      await act(async () => {
        void result.current.confirmRuntimeRefresh();
      });
      expect(result.current.refresh.phase).toBe("refreshing");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(31_000);
      });
      expect(result.current.refresh.phase).toBe("failed");
      expect(result.current.refresh.error).toContain("卡死");

      await act(async () => {
        resolveRefresh({
          outcome: "completed",
          configStatus: "ready",
          paginatedHistoryStatus: "ready",
          rendererCompatibilityStatus: "ready",
          rendererCompatibilityMessage: null,
          forceTerminated: false,
          closedProcessCount: 2,
          repairedHistoryRolloutCount: 0,
          repairedHistoryDuplicateCount: 0,
        });
        await Promise.resolve();
      });
      expect(result.current.refresh.phase).toBe("failed");
    } finally {
      vi.useRealTimers();
    }
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
