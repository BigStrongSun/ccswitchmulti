import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  syncCodexMultiRouterPlanWithProviders,
  syncCodexMultiRouterPlansAfterProviderChange,
} from "@/lib/codexMultiRouterSync";

// 构造测试用 provider；只填同步逻辑需要读取的字段。
function provider(overrides: Partial<Provider>): Provider {
  return {
    id: overrides.id ?? "provider",
    name: overrides.name ?? "Provider",
    category: overrides.category,
    settingsConfig: overrides.settingsConfig ?? {},
    meta: overrides.meta,
  };
}

describe("codexMultiRouterSync", () => {
  it("重建 catalog 时排除停用 route 的模型", () => {
    const enabled = provider({
      id: "qwen-enabled",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    });
    const disabled = provider({
      id: "openai-disabled",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    });
    const plan = provider({
      id: "router",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "qwen3.6" }, { model: "gpt-5.5" }],
          spawnAgentModels: ["qwen3.6", "gpt-5.5"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "qwen-route",
              enabled: true,
              targetProviderId: enabled.id,
              match: { models: ["qwen3.6"], prefixes: [] },
              upstream: {},
            },
            {
              id: "openai-route",
              enabled: false,
              targetProviderId: disabled.id,
              match: { models: ["gpt-5.5"], prefixes: ["gpt-"] },
              upstream: {},
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [enabled.id, enabled],
        [disabled.id, disabled],
      ]),
    );

    expect(
      synced?.plan.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["qwen3.6"]);
    expect(synced?.plan.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "qwen3.6",
    ]);
  });

  it("把旧版内联默认 OAuth route 迁移到 canonical provider 并同步 5.6", () => {
    const official = provider({
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "gpt-5.5" },
            { model: "gpt-5.6-luna" },
            { model: "gpt-5.6-sol" },
            { model: "gpt-5.6-terra" },
          ],
        },
      },
    });
    const backup = provider({
      id: "codex-official-backup",
      name: "OpenAI Official Backup",
      category: "official",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    });
    const plan = provider({
      id: "legacy-router",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "gpt-5.5" }],
          spawnAgentModels: ["gpt-5.5"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "legacy-official",
              match: { models: ["gpt-5.5"], prefixes: ["gpt-"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: { source: "managed_codex_oauth" },
              },
            },
          ],
        },
      },
    });
    const providersById = new Map([
      [official.id, official],
      [backup.id, backup],
      [plan.id, plan],
    ]);

    const synced = syncCodexMultiRouterPlanWithProviders(plan, providersById);

    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].targetProviderId,
    ).toBe("codex-official");
    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].match.models,
    ).toEqual(["gpt-5.5", "gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"]);
    expect(
      synced?.plan.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["gpt-5.5", "gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"]);
    expect(
      syncCodexMultiRouterPlanWithProviders(synced!.plan, providersById),
    ).toBeNull();
  });

  it("旧版内联 OAuth route 有账号时只迁移到同账号 provider", () => {
    const accountA = provider({
      id: "codex-official",
      name: "OpenAI Account A",
      category: "official",
      meta: {
        authBinding: {
          source: "managed_codex_oauth",
          accountId: "account-a",
        },
      },
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    });
    const accountB = provider({
      id: "codex-official-account-b",
      name: "OpenAI Account B",
      category: "official",
      meta: {
        authBinding: {
          source: "managed_codex_oauth",
          accountId: "account-b",
        },
      },
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.6-sol" }] },
      },
    });
    const plan = provider({
      id: "legacy-router",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "legacy-official",
              match: { models: ["gpt-5.5"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: {
                  source: "managed_codex_oauth",
                  accountId: "account-b",
                },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [accountA.id, accountA],
        [accountB.id, accountB],
        [plan.id, plan],
      ]),
    );

    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].targetProviderId,
    ).toBe(accountB.id);
    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].match.models,
    ).toEqual(["gpt-5.6-sol"]);
  });

  it("旧版内联 OAuth route 的账号没有匹配 provider 时保持原配置", () => {
    const official = provider({
      id: "codex-official",
      name: "OpenAI Account A",
      category: "official",
      meta: {
        authBinding: {
          source: "managed_codex_oauth",
          accountId: "account-a",
        },
      },
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.6-sol" }] },
      },
    });
    const plan = provider({
      id: "legacy-router",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "legacy-official",
              match: { models: ["gpt-5.5"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: {
                  source: "managed_codex_oauth",
                  accountId: "account-missing",
                },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [official.id, official],
        [plan.id, plan],
      ]),
    );

    const nextPlan = synced?.plan ?? plan;
    expect(
      nextPlan.settingsConfig.codexRouting.routes[0].targetProviderId,
    ).toBe(undefined);
    expect(nextPlan.settingsConfig.codexRouting.routes[0].match.models).toEqual(
      ["gpt-5.5"],
    );
  });

  it("同步 provider 保留模型变更到 route、总 catalog，并只剪枝子 Agent 候选", () => {
    const deepseek = provider({
      id: "deepseek",
      name: "DeepSeek",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "deepseek-chat" },
            { model: "deepseek-reasoner" },
            { model: "deepseek-v4-flash" },
          ],
        },
      },
    });
    const qwen = provider({
      id: "qwen",
      name: "Qwen",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    });
    const plan = provider({
      id: "router",
      name: "Codex MultiRouter",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "deepseek-chat" },
            { model: "old-removed-model" },
            { model: "qwen3.6" },
          ],
          spawnAgentModels: ["old-removed-model", "qwen3.6"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "router-deepseek",
              targetProviderId: "deepseek",
              match: { models: ["deepseek-chat", "old-removed-model"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
              },
            },
            {
              id: "router-qwen",
              targetProviderId: "qwen",
              match: { models: ["qwen3.6"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [deepseek.id, deepseek],
        [qwen.id, qwen],
        [plan.id, plan],
      ]),
    );

    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].match.models,
    ).toEqual(["deepseek-chat", "deepseek-reasoner", "deepseek-v4-flash"]);
    expect(
      synced?.plan.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual([
      "deepseek-chat",
      "deepseek-reasoner",
      "deepseek-v4-flash",
      "qwen3.6",
    ]);
    expect(synced?.plan.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "qwen3.6",
    ]);
    expect(synced?.removedSpawnAgentModels).toEqual(["old-removed-model"]);
  });

  it("同步 provider 模型变更时保留已保存 route 的别名 modelMap", () => {
    const relay = provider({
      id: "relay",
      name: "Relay",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "gpt-5.5", contextWindow: 272000 },
            { model: "gpt-5.4-mini", contextWindow: 128000 },
          ],
        },
      },
    });
    const plan = provider({
      id: "router",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "gpt-5.5-relay", upstreamModel: "gpt-5.5" }],
          spawnAgentModels: ["gpt-5.5-relay"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "router-relay",
              targetProviderId: "relay",
              match: { models: ["gpt-5.5-relay"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
                modelMap: { "gpt-5.5-relay": "gpt-5.5" },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [relay.id, relay],
        [plan.id, plan],
      ]),
    );

    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].match.models,
    ).toEqual(["gpt-5.5-relay", "gpt-5.4-mini"]);
    expect(
      synced?.plan.settingsConfig.codexRouting.routes[0].upstream.modelMap,
    ).toEqual({ "gpt-5.5-relay": "gpt-5.5" });
    expect(synced?.plan.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "gpt-5.5-relay",
    ]);
    expect(synced?.removedSpawnAgentModels).toEqual([]);
    expect(synced?.plan.settingsConfig.modelCatalog.models).toEqual([
      {
        model: "gpt-5.5-relay",
        upstreamModel: "gpt-5.5",
        displayName: "gpt-5.5-relay",
        contextWindow: 272000,
      },
      {
        model: "gpt-5.4-mini",
        upstreamModel: "gpt-5.4-mini",
        contextWindow: 128000,
      },
    ]);
  });

  it("目标 provider 目录暂时为空时不清空第三方 GPT 别名 route", () => {
    const official = provider({
      id: "official",
      name: "OpenAI Official",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "gpt-5.5", contextWindow: 300000 }],
        },
      },
    });
    const relay = provider({
      id: "relay",
      name: "Relay",
      settingsConfig: {
        modelCatalog: { models: [] },
      },
    });
    const plan = provider({
      id: "router",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "gpt-5.5", contextWindow: 272000 },
            {
              model: "gpt-5.5-relay",
              upstreamModel: "gpt-5.5",
              displayName: "Relay GPT",
              contextWindow: 272000,
            },
          ],
          spawnAgentModels: ["gpt-5.5-relay"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "router-official",
              targetProviderId: "official",
              match: { models: ["gpt-5.5"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: { source: "managed_codex_oauth" },
              },
            },
            {
              id: "router-relay",
              targetProviderId: "relay",
              match: { models: ["gpt-5.5-relay"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
                modelMap: { "gpt-5.5-relay": "gpt-5.5" },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [official.id, official],
        [relay.id, relay],
        [plan.id, plan],
      ]),
    );

    expect(synced).not.toBeNull();
    const nextPlan = synced?.plan ?? plan;
    expect(nextPlan.settingsConfig.codexRouting.routes[1].match.models).toEqual(
      ["gpt-5.5-relay"],
    );
    expect(
      nextPlan.settingsConfig.codexRouting.routes[1].upstream.modelMap,
    ).toEqual({ "gpt-5.5-relay": "gpt-5.5" });
    expect(nextPlan.settingsConfig.modelCatalog.models).toEqual([
      { model: "gpt-5.5", upstreamModel: "gpt-5.5", contextWindow: 300000 },
      {
        model: "gpt-5.5-relay",
        upstreamModel: "gpt-5.5",
        displayName: "Relay GPT",
        contextWindow: 272000,
      },
    ]);
    expect(nextPlan.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "gpt-5.5-relay",
    ]);
    expect(synced?.removedSpawnAgentModels).toEqual([]);
  });

  it("同步 provider 模型变更时修复官方和中转的同名 exact route", () => {
    const official = provider({
      id: "official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "gpt-5.5", contextWindow: 300000 }],
        },
      },
    });
    const relay = provider({
      id: "relay",
      name: "Relay GPT",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "gpt-5.5",
              displayName: "Relay GPT 5.5",
              contextWindow: 272000,
            },
          ],
        },
      },
    });
    const plan = provider({
      id: "router",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "gpt-5.5", contextWindow: 300000 }],
          spawnAgentModels: ["gpt-5.5"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "router-official",
              targetProviderId: "official",
              match: { models: ["gpt-5.5"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: { source: "managed_codex_oauth" },
              },
            },
            {
              id: "router-relay",
              targetProviderId: "relay",
              match: { models: ["gpt-5.5"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [official.id, official],
        [relay.id, relay],
        [plan.id, plan],
      ]),
    );

    expect(synced).not.toBeNull();
    const routes = synced?.plan.settingsConfig.codexRouting.routes ?? [];
    expect(routes[0].match.models).toEqual(["gpt-5.5"]);
    expect(routes[1].match.models).toEqual(["gpt-5.5-relay-gpt"]);
    expect(routes[1].upstream.modelMap).toEqual({
      "gpt-5.5-relay-gpt": "gpt-5.5",
    });
    expect(synced?.plan.settingsConfig.modelCatalog.models).toEqual([
      { model: "gpt-5.5", upstreamModel: "gpt-5.5", contextWindow: 300000 },
      {
        model: "gpt-5.5-relay-gpt",
        upstreamModel: "gpt-5.5",
        displayName: "Relay GPT 5.5",
        contextWindow: 272000,
      },
    ]);
    expect(synced?.removedSpawnAgentModels).toEqual([]);
  });

  it("同步 provider 模型变更时把新增第三方 GPT alias 加回第三方 route", () => {
    const official = provider({
      id: "official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "gpt-5.5", contextWindow: 300000 }],
        },
      },
    });
    const longnows = provider({
      id: "longnows",
      name: "LongNows GPT",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "claude-opus-4-8", contextWindow: 200000 },
            {
              model: "gpt-5.5-longnows-gpt",
              upstreamModel: "gpt-5.5",
              displayName: "LongNows GPT",
              contextWindow: 272000,
            },
          ],
        },
      },
    });
    const plan = provider({
      id: "router",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "gpt-5.5", contextWindow: 300000 },
            { model: "claude-opus-4-8", contextWindow: 200000 },
          ],
          spawnAgentModels: ["claude-opus-4-8"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "router-official",
              targetProviderId: "official",
              match: { models: ["gpt-5.5"], prefixes: ["gpt"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: { source: "managed_codex_oauth" },
              },
            },
            {
              id: "router-longnows",
              targetProviderId: "longnows",
              match: { models: ["claude-opus-4-8"], prefixes: ["claude"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
              },
            },
          ],
        },
      },
    });

    const synced = syncCodexMultiRouterPlanWithProviders(
      plan,
      new Map([
        [official.id, official],
        [longnows.id, longnows],
        [plan.id, plan],
      ]),
    );

    expect(synced).not.toBeNull();
    const routes = synced?.plan.settingsConfig.codexRouting.routes ?? [];
    expect(routes[1].match.models).toEqual([
      "claude-opus-4-8",
      "gpt-5.5-longnows-gpt",
    ]);
    expect(routes[1].upstream.modelMap).toEqual({
      "gpt-5.5-longnows-gpt": "gpt-5.5",
    });
    expect(synced?.plan.settingsConfig.modelCatalog.models).toContainEqual({
      model: "gpt-5.5-longnows-gpt",
      upstreamModel: "gpt-5.5",
      displayName: "LongNows GPT",
      contextWindow: 272000,
    });
  });

  it("provider id 改名时同步 route 目标并按新 provider 目录重建", () => {
    const renamed = provider({
      id: "new-provider",
      name: "New Provider",
      settingsConfig: {
        modelCatalog: { models: [{ model: "new-model" }] },
      },
    });
    const plan = provider({
      id: "router",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "old-model" }],
          spawnAgentModels: ["old-model"],
        },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "router-old",
              targetProviderId: "old-provider",
              match: { models: ["old-model"] },
              upstream: {
                apiFormat: "openai_chat",
                auth: { source: "provider_config" },
              },
            },
          ],
        },
      },
    });

    const [synced] = syncCodexMultiRouterPlansAfterProviderChange(
      [renamed, plan],
      renamed,
      "old-provider",
    );

    expect(
      synced.plan.settingsConfig.codexRouting.routes[0].targetProviderId,
    ).toBe("new-provider");
    expect(
      synced.plan.settingsConfig.codexRouting.routes[0].match.models,
    ).toEqual(["new-model"]);
    expect(synced.plan.settingsConfig.modelCatalog.spawnAgentModels).toEqual(
      [],
    );
    expect(synced.removedSpawnAgentModels).toEqual(["old-model"]);
  });
});
