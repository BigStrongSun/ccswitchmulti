import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  ProtocolLabCancelled,
  type ProtocolLabAdapter,
  useProtocolLabWorkflow,
} from "./useProtocolLabWorkflow";

type Draft = { id: string; manual?: boolean };
type Preview = { plan: "single" | "split" | "blocked" };

function createAdapter(
  overrides: Partial<
    ProtocolLabAdapter<Draft, string, Preview, string, string, string, string>
  > = {},
): ProtocolLabAdapter<Draft, string, Preview, string, string, string, string> {
  return {
    requiresProbe: (_draft, receiptIds) => receiptIds.length === 0,
    isManual: (draft) => draft.manual === true,
    preflight: vi.fn(async (_draft, onProgress) => {
      onProgress("baseline");
      return {
        outcome: "probe-outcome",
        receiptIds: ["receipt-one"],
        adaptationPreview: "adaptation-one",
      };
    }),
    prepare: vi.fn(async (): Promise<Preview> => ({ plan: "split" })),
    plan: (preview) => preview.plan,
    commit: vi.fn(async () => ({
      outcome: "commit-outcome",
      snapshot: "snapshot-one",
      projectionWarning: false,
    })),
    isDependencyChanged: (error) =>
      error instanceof Error && error.message === "dependency_changed",
    ...overrides,
  };
}

describe("useProtocolLabWorkflow", () => {
  it("commits an automatic Split plan directly with accept_auto", async () => {
    const adapter = createAdapter();
    const { result } = renderHook(() => useProtocolLabWorkflow(adapter));
    let savePromise!: Promise<string>;

    act(() => {
      savePromise = result.current.save({ id: "provider-one" });
    });
    expect(result.current.state.phase).toBe("awaiting_probe_consent");

    await act(async () => {
      await result.current.confirmProbe();
      await savePromise;
    });

    expect(adapter.commit).toHaveBeenCalledWith(
      { id: "provider-one" },
      ["receipt-one"],
      { plan: "split" },
      "accept_auto",
    );
    expect(result.current.state.phase).toBe("committed");
  });

  it("re-probes once after a dependency change within the same consent", async () => {
    const commit = vi
      .fn()
      .mockRejectedValueOnce(new Error("dependency_changed"))
      .mockResolvedValueOnce({
        outcome: "commit-outcome",
        snapshot: "snapshot-two",
        projectionWarning: false,
      });
    const adapter = createAdapter({ commit });
    const { result } = renderHook(() => useProtocolLabWorkflow(adapter));
    let savePromise!: Promise<string>;

    act(() => {
      savePromise = result.current.save({ id: "provider-one" });
    });
    await act(async () => {
      await result.current.confirmProbe();
      await savePromise;
    });

    expect(adapter.preflight).toHaveBeenCalledTimes(2);
    expect(adapter.commit).toHaveBeenCalledTimes(2);
    expect(result.current.state.staleRetryCount).toBe(1);
    expect(result.current.state.phase).toBe("committed");
  });

  it("keeps projection failure as a committed warning", async () => {
    const adapter = createAdapter({
      requiresProbe: () => false,
      commit: vi.fn(async () => ({
        outcome: "commit-outcome",
        snapshot: "snapshot-warning",
        projectionWarning: true,
        projectionErrorCode: "projection_pending",
      })),
    });
    const { result } = renderHook(() => useProtocolLabWorkflow(adapter));

    await act(async () => {
      await result.current.save({ id: "provider-one" }, ["receipt-one"]);
    });

    expect(result.current.state.phase).toBe("committed_projection_warning");
    expect(result.current.state.errorCode).toBe("projection_pending");
  });

  it("resets stale probe output and rejects an active operation", async () => {
    const adapter = createAdapter();
    const { result } = renderHook(() => useProtocolLabWorkflow(adapter));

    let firstValidation!: Promise<string>;
    act(() => {
      firstValidation = result.current.validate({ id: "provider-one" });
    });
    await act(async () => {
      await result.current.confirmProbe();
      await firstValidation;
    });
    expect(result.current.probeOutcome).toBe("probe-outcome");

    let pendingValidation!: Promise<string>;
    act(() => {
      pendingValidation = result.current.validate({ id: "provider-two" });
      pendingValidation.catch(() => undefined);
    });
    act(() => {
      result.current.reset({ id: "provider-three" });
    });

    await expect(pendingValidation).rejects.toBeInstanceOf(
      ProtocolLabCancelled,
    );
    expect(result.current.probeOutcome).toBeNull();
    expect(result.current.state.phase).toBe("idle");
    expect(result.current.state.draft).toEqual({ id: "provider-three" });
  });
});
