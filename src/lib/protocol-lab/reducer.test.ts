import { describe, expect, it } from "vitest";

import { createProtocolLabState, protocolLabReducer } from "./reducer";

describe("protocolLabReducer", () => {
  it("commits an automatic split without entering a confirmation state", () => {
    let state = createProtocolLabState<{ id: string }, { kind: string }>();
    state = protocolLabReducer(state, { type: "snapshot_loading" });
    state = protocolLabReducer(state, {
      type: "snapshot_loaded",
      snapshot: { id: "provider-a" },
    });
    state = protocolLabReducer(state, {
      type: "save_requested",
      requiresProbe: true,
    });
    expect(state.phase).toBe("awaiting_probe_consent");
    state = protocolLabReducer(state, { type: "probe_consented" });
    expect(state.phase).toBe("probing");
    state = protocolLabReducer(state, {
      type: "probe_succeeded",
      receiptIds: ["receipt-a", "receipt-b"],
      adaptationPreview: { kind: "mixed" },
    });
    expect(state.phase).toBe("preparing");
    state = protocolLabReducer(state, {
      type: "prepare_succeeded",
      preview: { kind: "split" },
      plan: "split",
    });
    expect(state.phase).toBe("committing");
    state = protocolLabReducer(state, {
      type: "commit_succeeded",
      snapshot: { id: "provider-a" },
      projectionWarning: false,
    });
    expect(state.phase).toBe("committed");
  });

  it("keeps the draft and evidence when prepare is blocked", () => {
    let state = createProtocolLabState<{ id: string }, { kind: string }>({
      draft: { id: "provider-a" },
    });
    state = protocolLabReducer(state, {
      type: "probe_succeeded",
      receiptIds: ["receipt-a"],
      adaptationPreview: { kind: "partial" },
    });
    state = protocolLabReducer(state, {
      type: "prepare_succeeded",
      preview: { kind: "blocked" },
      plan: "blocked",
    });

    expect(state.phase).toBe("blocked");
    expect(state.draft).toEqual({ id: "provider-a" });
    expect(state.receiptIds).toEqual(["receipt-a"]);
    expect(state.adaptationPreview).toEqual({ kind: "partial" });
  });

  it("automatically retries one dependency change and stops the loop on the second", () => {
    let state = createProtocolLabState<{ id: string }, never>({
      draft: { id: "provider-a" },
    });
    state = protocolLabReducer(state, { type: "dependency_changed" });
    expect(state.phase).toBe("stale_retry");
    expect(state.staleRetryCount).toBe(1);
    state = protocolLabReducer(state, { type: "stale_retry_started" });
    expect(state.phase).toBe("probing");
    state = protocolLabReducer(state, { type: "dependency_changed" });
    expect(state.phase).toBe("failed");
    expect(state.errorCode).toBe("dependency_changed_twice");
  });

  it("reports projection failure as committed with a warning", () => {
    let state = createProtocolLabState<{ id: string }, never>({
      draft: { id: "provider-a" },
    });
    state = protocolLabReducer(state, {
      type: "commit_succeeded",
      snapshot: { id: "provider-a" },
      projectionWarning: true,
      projectionErrorCode: "projection_pending",
    });

    expect(state.phase).toBe("committed_projection_warning");
    expect(state.errorCode).toBe("projection_pending");
    expect(state.snapshot).toEqual({ id: "provider-a" });
  });

  it("marks evidence stale on draft changes without deleting it", () => {
    let state = createProtocolLabState<{ id: string }, { kind: string }>({
      draft: { id: "provider-a" },
      adaptationPreview: { kind: "verified" },
      receiptIds: ["receipt-a"],
    });
    state = protocolLabReducer(state, {
      type: "draft_changed",
      draft: { id: "provider-b" },
    });

    expect(state.phase).toBe("draft_dirty");
    expect(state.evidenceStale).toBe(true);
    expect(state.adaptationPreview).toEqual({ kind: "verified" });
    expect(state.receiptIds).toEqual([]);
  });
});
