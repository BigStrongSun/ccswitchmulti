import { describe, expect, it, vi } from "vitest";

const resolveMock = vi.fn();
vi.mock("@/lib/presetCatalog", () => ({
  resolvePresetCatalogContextWindow: (...args: unknown[]) =>
    resolveMock(...args),
}));

import {
  inferCodexModelContextWindow,
  resolveFetchedCodexModelContextWindow,
} from "./codexModelContext";

describe("inferCodexModelContextWindow preset catalog integration", () => {
  it("uses the plan override for the official Codex subscription source", () => {
    resolveMock.mockReturnValue(258400);
    const context = inferCodexModelContextWindow("gpt-5.5", {
      providerName: "OpenAI Official",
      websiteUrl: "https://chatgpt.com/codex",
    });
    expect(context).toBe(258400);
    // 官方订阅必须带 plan 名查询
    expect(resolveMock).toHaveBeenCalledWith("gpt-5.5", "openai-codex-plan");
  });

  it("queries baseline (no plan) for non-official sources", () => {
    resolveMock.mockReturnValue(1050000);
    const context = inferCodexModelContextWindow("gpt-5.5", {
      providerName: "Some Aggregator",
      baseUrl: "https://api.example.com/v1",
    });
    expect(context).toBe(1050000);
    expect(resolveMock).toHaveBeenCalledWith("gpt-5.5", undefined);
  });

  it("falls back to hardcoded presets when the catalog misses", () => {
    resolveMock.mockReturnValue(undefined);
    // deepseek 别名映射是既有硬编码兜底，目录未命中时仍要生效。
    const context = inferCodexModelContextWindow("deepseek-chat", {
      providerName: "DeepSeek",
    });
    expect(context).toBe(1000000);
  });

  it("keeps fetched explicit value above catalog inference", () => {
    resolveMock.mockReturnValue(258400);
    const context = resolveFetchedCodexModelContextWindow(
      { id: "gpt-5.5", contextWindow: 999999 },
      { providerName: "OpenAI Official" },
    );
    expect(context).toBe(999999);
  });
});
