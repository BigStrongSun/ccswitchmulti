import { describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const bundle = {
  schemaVersion: 1,
  version: "2026.08.23",
  generatedAt: "2026-08-23T00:00:00Z",
  providers: {
    openai: { api: "https://api.openai.com/v1" },
  },
  baseline: {
    "openai/gpt-5.5": {
      name: "GPT-5.5",
      limit: { context: 1050000, input: 922000, output: 128000 },
      cost: { input: 5, output: 30 },
    },
    "anthropic/claude-sonnet-4-5": {
      name: "Claude Sonnet 4.5",
      limit: { context: 200000, output: 64000 },
    },
  },
  plans: {
    "openai-codex-plan": {
      "gpt-5.5": {
        base_model: "openai/gpt-5.5",
        plan: "openai-codex-plan",
        limit: { context: 272000, effective_context_percent: 95 },
        cost: { input: 0, output: 0 },
      },
    },
  },
  deployments: {
    "shared/openai-enterprise": {
      base_model: "openai/gpt-5.5",
      model_alias: "gpt-5.5-enterprise",
      transport: { api_format: "openai_responses" },
    },
  },
};

// 每个用例拿一个全新模块实例（模块级缓存随实例重置），避免跨用例串扰。
async function freshModule(response: unknown) {
  vi.resetModules();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(response);
  return import("./presetCatalog");
}

describe("loadPresetCatalog", () => {
  it("loads and caches the bundle", async () => {
    const mod = await freshModule(bundle);
    const loaded = await mod.loadPresetCatalog();
    expect(loaded).toEqual(bundle);
    await mod.loadPresetCatalog();
    // 第二次不触发 invoke（命中缓存）
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("returns null when the backend has no bundle", async () => {
    const mod = await freshModule(null);
    const loaded = await mod.loadPresetCatalog();
    expect(loaded).toBeNull();
  });

  it("returns null on invoke failure", async () => {
    const mod = await freshModule(null);
    invokeMock.mockRejectedValue(new Error("boom"));
    const loaded = await mod.loadPresetCatalog();
    expect(loaded).toBeNull();
  });
});

describe("resolvePresetCatalogContextWindow", () => {
  it("resolves plan override with effective percent", async () => {
    const mod = await freshModule(bundle);
    await mod.loadPresetCatalog();
    // 272000 * 95 / 100 = 258400
    expect(mod.resolvePresetCatalogContextWindow("gpt-5.5", "openai-codex-plan")).toBe(
      258400,
    );
  });

  it("does not fall back to baseline when plan is requested but missing", async () => {
    const mod = await freshModule(bundle);
    await mod.loadPresetCatalog();
    expect(
      mod.resolvePresetCatalogContextWindow("claude-sonnet-4-5", "openai-codex-plan"),
    ).toBeUndefined();
  });

  it("resolves baseline by model id across providers", async () => {
    const mod = await freshModule(bundle);
    await mod.loadPresetCatalog();
    expect(mod.resolvePresetCatalogContextWindow("gpt-5.5")).toBe(1050000);
    expect(mod.resolvePresetCatalogContextWindow("claude-sonnet-4-5")).toBe(200000);
    expect(mod.resolvePresetCatalogContextWindow("gpt-9.9")).toBeUndefined();
  });

  it("returns undefined before the bundle is loaded", async () => {
    // 未加载（缓存为空）时同步查询必须安全返回 undefined。
    const mod = await freshModule(bundle);
    expect(mod.resolvePresetCatalogContextWindow("gpt-5.5")).toBeUndefined();
  });
});

describe("resolvePresetCatalogEntry", () => {
  it("exposes a fully merged shared deployment entry", async () => {
    const mod = await freshModule(bundle);
    await mod.loadPresetCatalog();

    expect(
      mod.resolvePresetCatalogEntry(
        "openai",
        "gpt-5.5",
        "openai-codex-plan",
        "shared/openai-enterprise",
      ),
    ).toMatchObject({
      limit: { context: 272000, input: 922000 },
      model_alias: "gpt-5.5-enterprise",
      transport: { api_format: "openai_responses" },
    });
  });
});
