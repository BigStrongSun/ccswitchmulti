import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  findModelSource,
  isUnknownReasoningCapabilityError,
  readUnknownReasoningBlocker,
  readUnknownReasoningModels,
  unknownReasoningBlockerDetail,
} from "@/utils/codexSubagentReasoningBlocker";

const CODE = "unknown_reasoning_capability_requires_declaration";

function provider(model: string): Provider {
  return {
    id: `provider-${model}`,
    name: `Provider ${model}`,
    settingsConfig: { modelCatalog: { models: [{ model }] } },
  };
}

describe("readUnknownReasoningModels", () => {
  it("extracts the models the backend appended after the stable code", () => {
    const message = `无效输入: Codex subagent V2 configuration is incomplete (${CODE}): qwen3.8, glm-5.3`;
    expect(readUnknownReasoningModels(message)).toEqual(["qwen3.8", "glm-5.3"]);
  });

  it("tolerates the legacy message that only carried the code", () => {
    expect(
      readUnknownReasoningModels(
        `Codex subagent V2 configuration is incomplete (${CODE})`,
      ),
    ).toEqual([]);
  });

  it("returns nothing when the error is a different failure", () => {
    expect(
      readUnknownReasoningModels("codex_provider_set_batch_blocked"),
    ).toEqual([]);
    expect(isUnknownReasoningCapabilityError("candidate-invalid")).toBe(false);
  });

  it("trims, dedupes case-insensitively and accepts full-width separators", () => {
    const message = `(${CODE}): Qwen3.8、qwen3.8， DeepSeek-Flash`;
    expect(readUnknownReasoningModels(message)).toEqual([
      "Qwen3.8",
      "DeepSeek-Flash",
    ]);
  });
});

describe("readUnknownReasoningBlocker", () => {
  it("returns null for unrelated errors so callers keep the generic path", () => {
    expect(readUnknownReasoningBlocker("无效输入: something else")).toBeNull();
  });

  it("keeps the raw message for diagnostics", () => {
    const message = `无效输入: (${CODE}): qwen3.8`;
    expect(readUnknownReasoningBlocker(message)).toEqual({
      models: ["qwen3.8"],
      message,
    });
  });
});

describe("unknownReasoningBlockerDetail", () => {
  it("names every blocked model", () => {
    expect(unknownReasoningBlockerDetail(["qwen3.8", "glm-5.3"])).toContain(
      "qwen3.8、glm-5.3",
    );
  });

  it("still gives a next step when an older backend named no model", () => {
    expect(unknownReasoningBlockerDetail([])).toContain("模型目录声明能力");
  });
});

describe("findModelSource", () => {
  it("matches catalog models case-insensitively", () => {
    const sources = [provider("deepseek-flash"), provider("Qwen3.8")];
    expect(findModelSource(sources, "qwen3.8")?.id).toBe("provider-Qwen3.8");
  });

  it("returns undefined when no source declares the model", () => {
    expect(findModelSource([provider("deepseek-flash")], "glm-5.3")).toBeUndefined();
  });
});
