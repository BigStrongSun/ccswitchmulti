import { renderHook, waitFor } from "@testing-library/react";
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
    vi.mocked(useTauriEvent).mockImplementation((_name, handler) => {
      onEvent = handler as (payload: CodexConfigConsistencyReport) => void;
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
});
