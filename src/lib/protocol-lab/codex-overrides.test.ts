import { describe, expect, it } from "vitest";

import {
  migrateCodexProtocolOverrideKey,
  normalizeCodexPublicModelKey,
  setCodexProtocolOverride,
} from "./codex-overrides";

describe("Codex per-model protocol overrides", () => {
  it("normalizes public model keys exactly like the backend planner", () => {
    expect(normalizeCodexPublicModelKey("  Qwen3.8  ")).toBe("qwen3.8");
  });

  it("moves an override when the public model is renamed", () => {
    expect(
      migrateCodexProtocolOverrideKey(
        { "old-model": "openai_chat", untouched: "openai_responses" },
        "Old-Model",
        "New-Model",
      ),
    ).toEqual({
      "new-model": "openai_chat",
      untouched: "openai_responses",
    });
  });

  it("removes the explicit key when the model follows automatic selection", () => {
    expect(
      setCodexProtocolOverride(
        { qwen: "openai_chat", kimi: "openai_responses" },
        "QWEN",
        "follow_auto",
      ),
    ).toEqual({ kimi: "openai_responses" });
  });
});
