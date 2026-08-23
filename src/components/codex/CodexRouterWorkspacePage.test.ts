import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { providersApi } from "@/lib/api";
import {
  fetchCodexOauthModels,
  fetchModelsForConfig,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { CodexRoutingProjectionStatus } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
import type { Provider } from "@/types";
import type { PaginatedLogs, RequestLog } from "@/types/usage";
import {
  applyMultiRouterSettingsDraft,
  buildMultiRouterRuntimeStatus,
  buildCodexProxyBaseUrl,
  buildModelCatalogForRoutes,
  CodexRouterWorkspacePage,
  createRoutePolicyDraft,
  createDraftRoutingPlan,
  dedupeCodexRoutesBySemanticProvider,
  isRoutingPlan,
  mergeRoutePickerDraftIds,
  normalizeCodexRouteForSave,
  normalizeCodexRoutesForVisibleModelAliases,
  readCodexRouting,
  resolveCodexRouterAuthFacadeLabel,
  routeSummaryDisplayName,
  serializeCodexRouteV2,
  validateProxyListenDraft,
} from "./CodexRouterWorkspacePage";

const requestLogsFixture = vi.hoisted(() => ({
  value: { data: [], isLoading: false } as {
    data: PaginatedLogs | [];
    isLoading: boolean;
  },
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getGlobalProxyConfig: vi.fn().mockResolvedValue({
      listenAddress: "127.0.0.1",
      listenPort: 15721,
    }),
    diagnoseCodexMultiRouter: vi.fn(),
    unlockCodexModelPicker: vi.fn(),
  },
}));

vi.mock("@/lib/query/usage", () => ({
  usageKeys: {
    all: ["usage"],
  },
  useCodexSubagentUsageStats: () => ({
    data: {
      totals: {
        sessions: 0,
        inputTokens: 0,
        outputTokens: 0,
        totalTokens: 0,
      },
      agents: [],
      modelStats: [],
      providerModels: [],
    },
    isLoading: false,
    error: null,
  }),
  useRequestLogs: () => requestLogsFixture.value,
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    add: vi.fn(),
    update: vi.fn(),
    getAll: vi.fn(),
    inspectCodexMultiRouterProjection: vi.fn(),
    retryCodexMultiRouterProjection: vi.fn(),
    getCodexMultiRouterRevision: vi.fn(),
    previewCodexMultiRouterMigration: vi.fn(),
    applyCodexMultiRouterMigration: vi.fn(),
  },
}));

vi.mock("@/lib/api/auth", () => ({
  authApi: {
    getCodexAccountPoolPolicy: vi.fn().mockResolvedValue({
      enabled: false,
      entries: [],
    }),
  },
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchCodexOauthModels: vi.fn(),
  fetchModelsForConfig: vi.fn(),
}));

vi.mock("@/lib/api/codexSubagentV2", () => ({
  codexSubagentV2Api: {
    getReasoningCapabilities: vi.fn().mockResolvedValue({
      "deepseek-v4-flash": {
        supportKind: "effort_levels",
        source: "builtin",
        confidence: "confirmed",
        codexSelectableEfforts: [
          "none",
          "low",
          "medium",
          "high",
          "xhigh",
          "max",
        ],
        providerAcceptedEfforts: ["low", "high", "max"],
        providerDefaultEffort: "high",
        disableAllowed: true,
        effortMap: {
          low: "low",
          medium: "high",
          high: "high",
          xhigh: "high",
          max: "max",
        },
      },
    }),
    previewProfile: vi.fn().mockResolvedValue({
      providerKind: "third_party",
      requestedRoleName: "deepseek-v4-flash",
      effectiveRoleName: "deepseek-v4-flash",
      description: "Flash profile",
      developerInstructions: "Read only",
      nicknameCandidates: ["Flash"],
      model: "deepseek-v4-flash",
      modelProvider: "codex_model_router_v2",
      modelReasoningEffort: "medium",
      reasoningPolicy: "fixed",
      reasoningCapability: {
        supportKind: "effort_levels",
        source: "builtin",
        confidence: "confirmed",
        codexSelectableEfforts: [
          "none",
          "low",
          "medium",
          "high",
          "xhigh",
          "max",
        ],
        providerAcceptedEfforts: ["low", "high", "max"],
        providerDefaultEffort: "high",
        disableAllowed: true,
        effortMap: {
          low: "low",
          medium: "high",
          high: "high",
          xhigh: "high",
          max: "max",
        },
      },
      modelContextWindow: 128000,
      tomlPreview: 'model = "deepseek-v4-flash"',
      warnings: [],
    }),
    getProfileStatuses: vi.fn().mockResolvedValue({
      mode: "v2",
      generationSource: "configured_profiles",
      profiles: [
        {
          profileKey: "deepseek-v4-flash",
          model: "deepseek-v4-flash",
          providerKind: "third_party",
          enabled: true,
          routable: true,
          requestedRoleName: "deepseek-v4-flash",
          effectiveRoleName: "deepseek-v4-flash",
          modelProvider: "codex_model_router_v2",
          modelReasoningEffort: "medium",
          status: "generated",
          warnings: [],
        },
      ],
      warnings: [],
    }),
    updateProviderConfig: vi.fn(),
    initializeProviderConfig: vi.fn(),
    reconcileProviderProfiles: vi.fn(),
  },
}));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
};

/// 创建可手动完成的 Promise，用来稳定复现多个 provider 并发刷新时的返回顺序。
function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/// 为刷新类测试补齐真实的启用 route，避免把 provider 存在误当成已在当前方案中勾选。
function withEnabledProviderRoute(
  plan: Provider,
  provider: Provider,
): Provider {
  const models = provider.settingsConfig?.modelCatalog?.models ?? [];
  const route = normalizeCodexRouteForSave(
    {
      label: provider.name,
      enabled: true,
      targetProviderId: provider.id,
      modelSelection: { mode: "all" },
      match: {
        models: models.map((model: { model: string }) => model.model),
        prefixes: [],
      },
      upstream: { auth: { source: "provider_config" } },
    },
    0,
    new Set<string>(),
  );
  return {
    ...plan,
    settingsConfig: {
      ...plan.settingsConfig,
      codexRouting: {
        schemaVersion: 2,
        enabled: true,
        defaultRouteId: route.id,
        spawnAgentModels: models
          .map((model: { model: string }) => model.model)
          .slice(0, 5),
        routes: [serializeCodexRouteV2(route, 0)],
      },
    },
  };
}

function renderWorkspace(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    React.createElement(QueryClientProvider, { client: queryClient }, ui),
  );
}

/// 构造状态页所需的最小代理日志；测试只关心 Codex 最近一次真实转发是否成功。
function createCodexProxyLog(overrides: Partial<RequestLog> = {}): RequestLog {
  return {
    requestId: "req-1",
    providerId: "codex-router",
    providerName: "Codex MultiRouter",
    appType: "codex",
    model: "gpt-5.4-mini",
    requestModel: "gpt-5.4-mini",
    costMultiplier: "1",
    inputTokens: 1,
    outputTokens: 1,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    inputCostUsd: "0",
    outputCostUsd: "0",
    cacheReadCostUsd: "0",
    cacheCreationCostUsd: "0",
    totalCostUsd: "0",
    isStreaming: false,
    latencyMs: 100,
    statusCode: 200,
    createdAt: Date.now(),
    dataSource: "proxy",
    ...overrides,
  };
}

beforeEach(() => {
  requestLogsFixture.value = { data: [], isLoading: false };
  vi.mocked(fetchCodexOauthModels).mockReset();
  vi.mocked(fetchModelsForConfig).mockReset();
  vi.mocked(proxyApi.unlockCodexModelPicker).mockReset();
  vi.mocked(providersApi.add).mockResolvedValue(true);
  vi.mocked(providersApi.update).mockResolvedValue(true);
  vi.mocked(providersApi.getAll).mockResolvedValue({});
  vi.mocked(providersApi.inspectCodexMultiRouterProjection).mockResolvedValue({
    schemaVersion: 1,
    routerProviderId: "codex-multirouter",
    state: "ready",
    dependencyFingerprint: "test-fingerprint",
    generatedAt: "2026-08-21T00:00:00Z",
    warnings: [],
    routes: [],
    lastErrorCode: null,
    lastError: null,
  } satisfies CodexRoutingProjectionStatus);
  vi.mocked(providersApi.retryCodexMultiRouterProjection).mockResolvedValue({
    schemaVersion: 1,
    routerProviderId: "codex-multirouter",
    state: "ready",
    dependencyFingerprint: "test-fingerprint",
    generatedAt: "2026-08-21T00:00:00Z",
    warnings: [],
    routes: [],
    lastErrorCode: null,
    lastError: null,
  } satisfies CodexRoutingProjectionStatus);
  vi.mocked(providersApi.getCodexMultiRouterRevision).mockReset();
  vi.mocked(providersApi.previewCodexMultiRouterMigration).mockReset();
  vi.mocked(providersApi.applyCodexMultiRouterMigration).mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

it("没有 MultiRouter 方案时打开工作台不会读取 null settingsConfig", () => {
  const provider: Provider = {
    id: "valid-provider",
    name: "Valid Provider",
    settingsConfig: {},
  };

  expect(() =>
    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider],
        isProxyRunning: false,
        isCodexTakeoverActive: false,
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    ),
  ).not.toThrow();
  expect(screen.getByText("Codex 多模型路由工作台")).toBeInTheDocument();
});

describe("Codex MultiRouter workspace route persistence helpers", () => {
  it("uses the target Provider name in the default-route settings option for legacy UUID labels", async () => {
    const relay: Provider = {
      id: "relay-provider",
      name: "DeepSeek Relay",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "deepseek-v4-flash" }] },
      },
    };
    const legacyRouteId = "router-5626e6b9-33cb-4c3b-8d16-af8176e16209";
    const plan: Provider = {
      id: "legacy-router",
      name: "Legacy Router",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          defaultRouteId: legacyRouteId,
          routes: [
            {
              id: legacyRouteId,
              label: legacyRouteId,
              enabled: true,
              targetProviderId: relay.id,
              modelSelection: { mode: "all" },
              match: { models: ["deepseek-v4-flash"], prefixes: [] },
            },
          ],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [relay, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "编辑匹配规则" }),
      ).toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: "改名" }));

    const defaultRouteLabel = screen.getByText("默认路由", {
      selector: "label",
    });
    const defaultRoute =
      defaultRouteLabel.parentElement?.querySelector("select");
    expect(defaultRoute).not.toBeNull();
    expect(
      within(defaultRoute as HTMLElement).getByRole("option", {
        name: "DeepSeek Relay",
      }),
    ).toBeInTheDocument();
    expect(
      within(defaultRoute as HTMLElement).queryByRole("option", {
        name: legacyRouteId,
      }),
    ).not.toBeInTheDocument();
  });

  it("uses the target Provider name when a legacy route label is only its UUID", () => {
    expect(
      routeSummaryDisplayName(
        "router-5626e6b9-33cb-4c3b-8d16-af8176e16209",
        "router-5626e6b9-33cb-4c3b-8d16-af8176e16209",
        "DeepSeek Relay",
      ),
    ).toBe("DeepSeek Relay");
    expect(
      routeSummaryDisplayName(
        undefined,
        "router-5626e6b9-33cb-4c3b-8d16-af8176e16209",
        undefined,
      ),
    ).toBe("router-5626e6b9-33cb-4c3b-8d16-af8176e16209");
  });

  it("keeps an opaque legacy route label out of the persisted edit draft", () => {
    const routeId = "router-5626e6b9-33cb-4c3b-8d16-af8176e16209";
    const draft = createRoutePolicyDraft({
      id: routeId,
      route: {
        id: routeId,
        label: routeId,
        targetProviderId: "deepseek-relay",
      },
      provider: {
        id: "deepseek-relay",
        name: "DeepSeek Relay",
      },
      isExisting: true,
      matchModels: [],
      matchPrefixes: [],
    } as any);

    expect(draft.route.id).toBe(routeId);
    expect(draft.route.label).toBe(routeId);
    expect(serializeCodexRouteV2(draft.route, 0).label).toBe(routeId);
    expect(
      routeSummaryDisplayName(draft.route.label, routeId, "DeepSeek Relay"),
    ).toBe("DeepSeek Relay");
  });

  it("serializes only schema v2 route policy and drops inherited secrets and capabilities", () => {
    const route = serializeCodexRouteV2(
      {
        id: "qwen-route",
        label: "Qwen",
        enabled: true,
        targetProviderId: "qwen-provider",
        modelSelection: { mode: "include", models: ["qwen3.8"] },
        match: { models: ["Qwen 3.8"], prefixes: ["qwen"] },
        aliases: { "Qwen 3.8": "qwen3.8" },
        authPolicy: {
          source: "managed_codex_oauth",
          authProvider: "codex_oauth",
          accountId: "account-1",
        },
        upstream: {
          baseUrl: "https://must-not-persist.invalid/v1",
          apiFormat: "openai_chat",
          auth: { source: "provider_config" },
          modelMap: { "Qwen 3.8": "qwen3.8" },
        },
        capabilities: {
          textOnly: true,
          supportsReasoning: true,
        },
      },
      0,
    );

    expect(route).toEqual({
      id: "qwen-route",
      label: "Qwen",
      enabled: true,
      targetProviderId: "qwen-provider",
      modelSelection: { mode: "include", models: ["qwen3.8"] },
      matchPrefixes: ["qwen"],
      aliases: { "Qwen 3.8": "qwen3.8" },
      authPolicy: {
        source: "managed_codex_oauth",
        accountId: "account-1",
      },
    });
    expect(route).not.toHaveProperty("upstream");
    expect(route).not.toHaveProperty("capabilities");
  });

  it("edits and saves only schema v2 route policy with canonical include models", async () => {
    const qwen: Provider = {
      id: "codex-qwen-policy",
      name: "Qwen Policy",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "qwen-visible", upstreamModel: "qwen3.8" },
            { model: "qwen3.8-coder" },
          ],
        },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([qwen], [qwen]),
      qwen,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [qwen, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "编辑匹配规则" }));
    await user.selectOptions(
      screen.getByLabelText("模型选择范围：Qwen Policy"),
      "include",
    );
    expect(
      screen.getByLabelText("选择 canonical 模型 qwen-visible"),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText("选择 canonical 模型 qwen3.8"),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByLabelText("选择 canonical 模型 qwen3.8-coder"),
    );
    await user.clear(screen.getByLabelText("匹配前缀：Qwen Policy"));
    await user.type(screen.getByLabelText("匹配前缀：Qwen Policy"), "qwen, qw");
    await user.clear(screen.getByLabelText("可见别名映射：Qwen Policy"));
    await user.type(
      screen.getByLabelText("可见别名映射：Qwen Policy"),
      "Qwen Latest=qwen-visible",
    );
    await user.selectOptions(
      screen.getByLabelText("认证策略：Qwen Policy"),
      "managed_codex_oauth",
    );
    await user.type(
      screen.getByLabelText("托管 OAuth 账号 ID：Qwen Policy"),
      "oauth-account-1",
    );

    expect(screen.queryByLabelText(/Base URL/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/API Key/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/上游协议/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/能力开关/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => expect(providersApi.update).toHaveBeenCalled());
    const savedPlan = vi.mocked(providersApi.update).mock.calls.at(-1)?.[0];
    const savedRoute = (
      savedPlan?.settingsConfig.codexRouting as { routes?: unknown[] }
    )?.routes?.[0] as Record<string, unknown> | undefined;
    expect(savedRoute).toMatchObject({
      label: "Qwen Policy",
      targetProviderId: qwen.id,
      modelSelection: { mode: "include", models: ["qwen-visible"] },
      matchPrefixes: ["qwen", "qw"],
      aliases: { "Qwen Latest": "qwen-visible" },
      authPolicy: {
        source: "managed_codex_oauth",
        accountId: "oauth-account-1",
      },
    });
    expect(savedRoute).not.toHaveProperty("upstream");
    expect(savedRoute).not.toHaveProperty("capabilities");
  });

  it("blocks saving when an alias target is not a canonical provider model", async () => {
    const qwen: Provider = {
      id: "codex-qwen-invalid-alias",
      name: "Qwen Invalid Alias",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.8" }] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([qwen], [qwen]),
      qwen,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [qwen, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "编辑匹配规则" }));
    await user.type(
      screen.getByLabelText("可见别名映射：Qwen Invalid Alias"),
      "Broken=missing-model",
    );
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    expect(
      screen.getByText(
        "别名目标 missing-model 不属于目标 Provider 的 canonical 模型",
      ),
    ).toBeInTheDocument();
    expect(providersApi.update).not.toHaveBeenCalled();
  });

  it("shows a redacted migration preview before editing a schema v1 plan", async () => {
    const source: Provider = {
      id: "qwen",
      name: "Qwen",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://private.invalid/v1",
        auth: { OPENAI_API_KEY: "must-not-render" },
        modelCatalog: { models: [{ model: "qwen3.8" }] },
      },
    };
    const legacyPlan: Provider = {
      id: "legacy-router",
      name: "Legacy Router",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "qwen-route",
              targetProviderId: "qwen",
              match: { models: ["qwen3.8"] },
              upstream: {
                apiFormat: "openai_chat",
                apiKey: "legacy-secret",
                auth: { source: "provider_config" },
              },
            },
          ],
        },
      },
    };
    vi.mocked(providersApi.getCodexMultiRouterRevision).mockResolvedValue(
      "revision-1",
    );
    vi.mocked(providersApi.previewCodexMultiRouterMigration).mockResolvedValue({
      schemaVersion: 2,
      providerId: legacyPlan.id,
      expectedRevision: "revision-1",
      planToken: "opaque-plan-token",
      diff: {
        removedRouteFields: ["upstream.apiFormat", "upstream.apiKey"],
        createdProviderIds: ["qwen-migrated"],
        changedRouteIds: ["qwen-route"],
      },
      warnings: ["需要创建迁移 Provider"],
      generatedProviders: [
        {
          id: "qwen-migrated",
          name: "Qwen migrated",
          migrationGenerated: true,
          sourceProviderId: "qwen",
        },
      ],
    });

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, legacyPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: legacyPlan.id,
        initialProviderId: legacyPlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "编辑匹配规则" }));

    expect(
      await screen.findByRole("heading", {
        name: "迁移旧 MultiRouter 到 schema v2",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("需要创建迁移 Provider")).toBeInTheDocument();
    expect(screen.getByText(/Qwen migrated/)).toBeInTheDocument();
    expect(screen.queryByText("legacy-secret")).not.toBeInTheDocument();
    expect(screen.queryByText("must-not-render")).not.toBeInTheDocument();
    expect(providersApi.update).not.toHaveBeenCalled();
  });

  it("normalizes missing and invalid subagent versions to V2 while preserving V1", () => {
    const planWith = (subagentVersion?: unknown): Provider => ({
      id: `router-${String(subagentVersion)}`,
      name: "Router",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          enabled: true,
          routes: [],
          ...(subagentVersion === undefined ? {} : { subagentVersion }),
        },
      },
    });

    expect((readCodexRouting(planWith()) as any)?.subagentVersion).toBe("v2");
    expect(
      (readCodexRouting(planWith("unexpected")) as any)?.subagentVersion,
    ).toBe("v2");
    expect((readCodexRouting(planWith("v1")) as any)?.subagentVersion).toBe(
      "v1",
    );
  });

  it("does not infer a legacy route target from an ambiguous shared model", () => {
    const providerA = {
      id: "relay-a",
      name: "Relay A",
      settingsConfig: {
        modelCatalog: { models: [{ model: "shared-model" }] },
      },
    } as Provider;
    const providerB = {
      id: "relay-b",
      name: "Relay B",
      settingsConfig: {
        modelCatalog: { models: [{ model: "shared-model" }] },
      },
    } as Provider;
    const routes = [
      {
        id: "legacy-route",
        label: "legacy-route",
        match: { models: ["shared-model"] },
      },
      {
        id: "relay-a-route",
        targetProviderId: "relay-a",
        match: { models: ["shared-model"] },
      },
    ];

    expect(
      dedupeCodexRoutesBySemanticProvider(routes, [providerA, providerB]),
    ).toEqual(routes);
  });

  it("按 Router 官方认证与账号池 Desktop 成员展示生成门面", () => {
    expect(
      resolveCodexRouterAuthFacadeLabel({ mode: "desktop_current_login" }),
    ).toBe("Desktop / 混合认证");
    expect(resolveCodexRouterAuthFacadeLabel({ mode: "managed_oauth" })).toBe(
      "CCSM 托管认证",
    );
    expect(
      resolveCodexRouterAuthFacadeLabel(
        { mode: "account_pool" },
        {
          enabled: true,
          entries: [
            {
              accountId: "native_codex_auth",
              enabled: true,
              reservePercent: 5,
            },
          ],
        },
      ),
    ).toBe("Desktop / 混合认证");
    expect(
      resolveCodexRouterAuthFacadeLabel(
        { mode: "account_pool" },
        {
          enabled: true,
          entries: [
            {
              accountId: "native_codex_auth",
              enabled: false,
              reservePercent: 5,
            },
          ],
        },
      ),
    ).toBe("CCSM 托管认证");
    expect(resolveCodexRouterAuthFacadeLabel({ mode: "account_pool" })).toBe(
      "待确认",
    );
  });

  it("refreshes an official OAuth Provider without rewriting its MultiRouter route", async () => {
    vi.mocked(fetchCodexOauthModels).mockResolvedValue([
      { id: "gpt-5.5", ownedBy: "openai", contextWindow: 272000 },
      { id: "gpt-5.6-sol", ownedBy: "openai", contextWindow: 272000 },
    ]);
    const provider: Provider = {
      id: "openai-official",
      name: "OpenAI Official",
      category: "official",
      meta: {
        providerType: "codex_oauth",
        authBinding: {
          source: "managed_codex_oauth",
          accountId: "account-56",
        },
      },
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "gpt-5.5",
              upstreamModel: "gpt-5.5",
              contextWindow: 272000,
            },
          ],
        },
      },
    };
    const planDraft = createDraftRoutingPlan([provider], [provider]);
    const route = normalizeCodexRouteForSave(
      {
        label: provider.name,
        targetProviderId: provider.id,
        match: { models: ["gpt-5.5"] },
      },
      0,
      new Set<string>(),
    );
    const plan: Provider = {
      ...planDraft,
      settingsConfig: {
        ...planDraft.settingsConfig,
        codexRouting: {
          enabled: true,
          defaultRouteId: route.id,
          routes: [route],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(fetchCodexOauthModels).toHaveBeenCalledWith("account-56"),
    );
    await waitFor(() => {
      const savedProvider = vi
        .mocked(providersApi.update)
        .mock.calls.map(([updated]) => updated)
        .find((updated) => updated.id === provider.id);
      expect(
        savedProvider?.settingsConfig?.modelCatalog?.models.map(
          (model: { model: string }) => model.model,
        ),
      ).toEqual(["gpt-5.5", "gpt-5.6-sol"]);

      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(([updated]) => updated.id === plan.id),
      ).toBe(false);
    });
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
  });

  it("refreshes only providers referenced by enabled routes", async () => {
    vi.mocked(fetchCodexOauthModels).mockResolvedValue([
      { id: "gpt-5.6-sol", ownedBy: "openai", contextWindow: 272000 },
    ]);
    const enabledProvider: Provider = {
      id: "openai-enabled",
      name: "OpenAI Enabled",
      category: "official",
      meta: {
        providerType: "codex_oauth",
        authBinding: {
          source: "managed_codex_oauth",
          accountId: "account-enabled",
        },
      },
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    };
    const disabledProvider: Provider = {
      id: "openai-disabled",
      name: "OpenAI Disabled",
      category: "official",
      meta: {
        providerType: "codex_oauth",
        authBinding: {
          source: "managed_codex_oauth",
          accountId: "account-disabled",
        },
      },
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.4" }] },
      },
    };
    const planDraft = createDraftRoutingPlan(
      [enabledProvider, disabledProvider],
      [enabledProvider, disabledProvider],
    );
    const plan: Provider = {
      ...planDraft,
      settingsConfig: {
        ...planDraft.settingsConfig,
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "enabled-route",
              enabled: true,
              targetProviderId: enabledProvider.id,
              match: { models: ["gpt-5.5"], prefixes: [] },
              upstream: {},
            },
            {
              id: "disabled-route",
              enabled: false,
              targetProviderId: disabledProvider.id,
              match: { models: ["gpt-5.4"], prefixes: [] },
              upstream: {},
            },
          ],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [enabledProvider, disabledProvider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(fetchCodexOauthModels).toHaveBeenCalledWith("account-enabled"),
    );
    expect(fetchCodexOauthModels).not.toHaveBeenCalledWith("account-disabled");
  });

  it("clears disabled provider refresh state and refreshes again after re-enabling", async () => {
    vi.mocked(fetchModelsForConfig).mockImplementation(
      () => new Promise(() => {}),
    );
    const provider: Provider = {
      id: "toggle-refresh-provider",
      name: "Toggle Refresh Provider",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://toggle-refresh.example/v1",
        auth: { OPENAI_API_KEY: "toggle-key" },
        modelCatalog: { models: [{ model: "toggle-model" }] },
      },
    };
    const basePlan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    /// 在同一工作台实例中切换 route，覆盖停用状态清理和重新启用刷新。
    function RefreshToggleHarness() {
      const [enabled, setEnabled] = React.useState(true);
      const routes = readCodexRouting(basePlan)?.routes ?? [];
      const plan: Provider = {
        ...basePlan,
        settingsConfig: {
          ...basePlan.settingsConfig,
          codexRouting: {
            ...readCodexRouting(basePlan),
            enabled: true,
            routes: routes.map((route) => ({ ...route, enabled })),
          },
        },
      };
      return React.createElement(
        React.Fragment,
        null,
        React.createElement(
          "button",
          { type: "button", onClick: () => setEnabled((value) => !value) },
          "切换测试路由",
        ),
        React.createElement(CodexRouterWorkspacePage, {
          providers: [provider, plan],
          isProxyRunning: true,
          isCodexTakeoverActive: true,
          activeProviderId: plan.id,
          initialProviderId: plan.id,
          initialTab: "routes",
          onEditProvider: vi.fn(),
          onDeletePlan: vi.fn(),
          onCreateProvider: vi.fn(),
        }),
      );
    }

    renderWorkspace(React.createElement(RefreshToggleHarness));
    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    expect(screen.getByText("正在读取模型列表...")).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "切换测试路由" }));
    await waitFor(() =>
      expect(screen.queryByText("正在读取模型列表...")).not.toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "切换测试路由" }));
    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(2));
  });

  it("invalidates an in-flight refresh before saving a disabled route", async () => {
    const modelRefresh = createDeferred<FetchedModel[]>();
    const routeSave = createDeferred<boolean>();
    vi.mocked(fetchModelsForConfig).mockReturnValue(modelRefresh.promise);
    const provider: Provider = {
      id: "save-race-provider",
      name: "Save Race Provider",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://save-race.example/v1",
        auth: { OPENAI_API_KEY: "save-race-key" },
        modelCatalog: { models: [{ model: "old-model" }] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );
    vi.mocked(providersApi.update).mockImplementation((updated) =>
      updated.id === plan.id ? routeSave.promise : Promise.resolve(true),
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "编辑匹配规则" }));
    await user.click(screen.getByRole("button", { name: "已启用" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => {
      const savedPlan = vi
        .mocked(providersApi.update)
        .mock.calls.map(([updated]) => updated)
        .find((updated) => updated.id === plan.id);
      expect(savedPlan).toBeDefined();
      if (!savedPlan) throw new Error("未捕获到停用 route 的保存请求");
      expect(readCodexRouting(savedPlan)?.routes?.[0]?.enabled).toBe(false);
      expect(savedPlan.settingsConfig).not.toHaveProperty("modelCatalog");
    });

    await act(async () => {
      modelRefresh.resolve([
        { id: "late-model", ownedBy: "save-race", contextWindow: 128000 },
      ]);
      await Promise.resolve();
    });
    expect(
      vi
        .mocked(providersApi.update)
        .mock.calls.some(([updated]) => updated.id === provider.id),
    ).toBe(false);

    await act(async () => {
      routeSave.resolve(true);
      await Promise.resolve();
    });
  });

  it("keeps a legacy inline OAuth route read-only during Provider catalog refresh", async () => {
    vi.mocked(fetchCodexOauthModels).mockResolvedValue([
      { id: "gpt-5.5", ownedBy: "openai", contextWindow: 272000 },
      { id: "gpt-5.6-luna", ownedBy: "openai", contextWindow: 272000 },
      { id: "gpt-5.6-sol", ownedBy: "openai", contextWindow: 272000 },
      { id: "gpt-5.6-terra", ownedBy: "openai", contextWindow: 272000 },
    ]);
    const official: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    };
    const planDraft = createDraftRoutingPlan([], []);
    const legacyRoute = normalizeCodexRouteForSave(
      {
        label: official.name,
        match: { models: ["gpt-5.5"], prefixes: ["gpt-"] },
      },
      0,
      new Set<string>(),
    );
    delete legacyRoute.targetProviderId;
    legacyRoute.upstream!.auth = { source: "managed_codex_oauth" };
    const legacyPlan: Provider = {
      ...planDraft,
      id: "codex-openai-router",
      name: "OpenAI Multi-Model Router",
      settingsConfig: {
        ...planDraft.settingsConfig,
        modelCatalog: {
          models: [{ model: "gpt-5.5" }],
          spawnAgentModels: ["gpt-5.5"],
        },
        codexRouting: {
          enabled: true,
          defaultRouteId: legacyRoute.id,
          routes: [legacyRoute],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [official, legacyPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: legacyPlan.id,
        initialProviderId: legacyPlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(([updated]) => updated.id === official.id),
      ).toBe(true),
    );
    expect(
      vi
        .mocked(providersApi.update)
        .mock.calls.some(([updated]) => updated.id === legacyPlan.id),
    ).toBe(false);
  });

  it("finishes later provider refreshes after an earlier refresh rerenders the routes page", async () => {
    const firstRefresh = createDeferred<FetchedModel[]>();
    const secondRefresh = createDeferred<FetchedModel[]>();
    vi.mocked(fetchModelsForConfig)
      .mockReturnValueOnce(firstRefresh.promise)
      .mockReturnValueOnce(secondRefresh.promise);
    const providerA: Provider = {
      id: "codex-source-a",
      name: "Provider A",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://a.example/v1",
        auth: { OPENAI_API_KEY: "key-a" },
        modelCatalog: { models: [{ model: "old-a" }] },
      },
    };
    const providerB: Provider = {
      id: "codex-source-b",
      name: "Provider B",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://b.example/v1",
        auth: { OPENAI_API_KEY: "key-b" },
        modelCatalog: { models: [{ model: "old-b" }] },
      },
    };
    const plan = createDraftRoutingPlan(
      [providerA, providerB],
      [providerA, providerB],
    );
    const usedRouteIds = new Set<string>();
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: providerA.name,
          targetProviderId: providerA.id,
          match: { models: ["model-a"] },
        },
        0,
        usedRouteIds,
      ),
      normalizeCodexRouteForSave(
        {
          label: providerB.name,
          targetProviderId: providerB.id,
          match: { models: ["model-b"] },
        },
        1,
        usedRouteIds,
      ),
    ];
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          enabled: true,
          defaultRouteId: routes[0].id,
          routes,
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [providerA, providerB, routedPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: routedPlan.id,
        initialProviderId: routedPlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(2));

    firstRefresh.resolve([{ id: "model-a", ownedBy: null }]);
    await waitFor(() =>
      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(([provider]) => provider.id === providerA.id),
      ).toBe(true),
    );

    secondRefresh.resolve([{ id: "model-b", ownedBy: null }]);
    await waitFor(() =>
      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(([provider]) => provider.id === providerB.id),
      ).toBe(true),
    );
    await waitFor(() =>
      expect(screen.getAllByText("已读取并更新 1 个模型。")).toHaveLength(2),
    );
  });

  it("reports a later provider refresh error after an earlier refresh rerenders the routes page", async () => {
    const firstRefresh = createDeferred<FetchedModel[]>();
    const secondRefresh = createDeferred<FetchedModel[]>();
    vi.mocked(fetchModelsForConfig)
      .mockReturnValueOnce(firstRefresh.promise)
      .mockReturnValueOnce(secondRefresh.promise);
    const providerA: Provider = {
      id: "codex-error-source-a",
      name: "Provider A",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://a.example/v1",
        auth: { OPENAI_API_KEY: "key-a" },
        modelCatalog: { models: [{ model: "old-a" }] },
      },
    };
    const providerB: Provider = {
      id: "codex-error-source-b",
      name: "Provider B",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://b.example/v1",
        auth: { OPENAI_API_KEY: "key-b" },
        modelCatalog: { models: [{ model: "old-b" }] },
      },
    };
    const plan = createDraftRoutingPlan(
      [providerA, providerB],
      [providerA, providerB],
    );
    const usedRouteIds = new Set<string>();
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: providerA.name,
          targetProviderId: providerA.id,
          match: { models: ["model-a"] },
        },
        0,
        usedRouteIds,
      ),
      normalizeCodexRouteForSave(
        {
          label: providerB.name,
          targetProviderId: providerB.id,
          match: { models: ["model-b"] },
        },
        1,
        usedRouteIds,
      ),
    ];
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          enabled: true,
          defaultRouteId: routes[0].id,
          routes,
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [providerA, providerB, routedPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: routedPlan.id,
        initialProviderId: routedPlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(2));

    firstRefresh.resolve([{ id: "model-a", ownedBy: null }]);
    await waitFor(() =>
      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(([provider]) => provider.id === providerA.id),
      ).toBe(true),
    );

    secondRefresh.reject(new Error("network timeout"));
    await waitFor(() =>
      expect(
        screen.getByText(
          "获取模型列表失败，请检查当前 provider 配置：network timeout",
        ),
      ).toBeInTheDocument(),
    );
  });

  it("restarts provider refresh when the api key changes and ignores the stale request", async () => {
    const staleRefresh = createDeferred<FetchedModel[]>();
    const currentRefresh = createDeferred<FetchedModel[]>();
    vi.mocked(fetchModelsForConfig)
      .mockReturnValueOnce(staleRefresh.promise)
      .mockReturnValueOnce(currentRefresh.promise);
    const provider: Provider = {
      id: "codex-keyed-source",
      name: "Keyed Source",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://keyed.example/v1",
        auth: { OPENAI_API_KEY: "old-key" },
        modelCatalog: { models: [{ model: "old-catalog" }] },
      },
    };
    const plan = createDraftRoutingPlan([provider], [provider]);
    const usedRouteIds = new Set<string>();
    const route = normalizeCodexRouteForSave(
      {
        label: provider.name,
        targetProviderId: provider.id,
        match: { models: ["new-model"] },
      },
      0,
      usedRouteIds,
    );
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          enabled: true,
          defaultRouteId: route.id,
          routes: [route],
        },
      },
    };
    const props = {
      providers: [provider, routedPlan],
      isProxyRunning: true,
      isCodexTakeoverActive: true,
      activeProviderId: routedPlan.id,
      initialProviderId: routedPlan.id,
      initialTab: "routes" as const,
      onEditProvider: vi.fn(),
      onDeletePlan: vi.fn(),
      onCreateProvider: vi.fn(),
    };

    const { rerender } = renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, props),
    );
    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));

    const providerWithNewKey: Provider = {
      ...provider,
      settingsConfig: {
        ...provider.settingsConfig,
        auth: { OPENAI_API_KEY: "new-key" },
      },
    };
    rerender(
      React.createElement(
        QueryClientProvider,
        {
          client: new QueryClient({
            defaultOptions: { queries: { retry: false } },
          }),
        },
        React.createElement(CodexRouterWorkspacePage, {
          ...props,
          providers: [providerWithNewKey, routedPlan],
        }),
      ),
    );
    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(2));

    staleRefresh.resolve([{ id: "stale-model", ownedBy: null }]);
    await Promise.resolve();
    expect(
      vi
        .mocked(providersApi.update)
        .mock.calls.some(([savedProvider]) =>
          JSON.stringify(savedProvider.settingsConfig).includes("stale-model"),
        ),
    ).toBe(false);

    currentRefresh.resolve([
      { id: "old-catalog", ownedBy: null, contextWindow: 131072 },
    ]);
    await waitFor(() =>
      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(
            ([savedProvider]) =>
              savedProvider.id === provider.id &&
              JSON.stringify(savedProvider.settingsConfig).includes("131072"),
          ),
      ).toBe(true),
    );
    expect(
      vi
        .mocked(providersApi.update)
        .mock.calls.some(([savedProvider]) =>
          JSON.stringify(savedProvider.settingsConfig).includes("stale-model"),
        ),
    ).toBe(false);
  });

  it("settles a provider refresh when the model fetch ipc never returns", async () => {
    vi.useFakeTimers();
    vi.mocked(fetchModelsForConfig).mockReturnValue(new Promise(() => {}));
    const provider: Provider = {
      id: "codex-timeout-source",
      name: "Timeout Source",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://timeout.example/v1",
        auth: { OPENAI_API_KEY: "key-timeout" },
        modelCatalog: { models: [{ model: "old-timeout" }] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(
      screen.getByText(
        "获取模型列表失败，请检查当前 provider 配置：模型列表读取或写回超过 30 秒，请检查网络、provider /models 端点或本地配置写入状态。",
      ),
    ).toBeInTheDocument();
  });

  it("settles a provider refresh when saving fetched models never returns", async () => {
    vi.useFakeTimers();
    const saveRefresh = createDeferred<boolean>();
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "model-from-upstream", ownedBy: null },
    ]);
    vi.mocked(providersApi.update).mockReturnValue(saveRefresh.promise);
    const provider: Provider = {
      id: "codex-save-timeout-source",
      name: "Save Timeout Source",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://save-timeout.example/v1",
        auth: { OPENAI_API_KEY: "key-save-timeout" },
        modelCatalog: { models: [{ model: "old-save-timeout" }] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
    expect(
      screen.getByText("已读取 1 个模型，正在写回本地配置..."),
    ).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(
      screen.getByText(
        "获取模型列表失败，请检查当前 provider 配置：模型列表读取或写回超过 30 秒，请检查网络、provider /models 端点或本地配置写入状态。",
      ),
    ).toBeInTheDocument();

    saveRefresh.resolve(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(
      screen.queryByText("已读取并更新 1 个模型。"),
    ).not.toBeInTheDocument();
  });

  it("refreshes visible route picker candidates after provider catalog save without parent refetch", async () => {
    const refresh = createDeferred<FetchedModel[]>();
    vi.mocked(fetchModelsForConfig).mockReturnValueOnce(refresh.promise);
    const provider: Provider = {
      id: "codex-empty-catalog-source",
      name: "Empty Catalog Source",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://empty-catalog.example/v1",
        auth: { OPENAI_API_KEY: "key-empty-catalog" },
        modelCatalog: { models: [] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "编辑匹配规则" }));
    expect(
      screen.getByText("未发现模型目录，保存后可在模型源补充目录"),
    ).toBeInTheDocument();

    refresh.resolve([{ id: "fresh-route-model", ownedBy: null }]);
    await waitFor(() =>
      expect(
        vi
          .mocked(providersApi.update)
          .mock.calls.some(
            ([savedProvider]) =>
              savedProvider.id === provider.id &&
              JSON.stringify(savedProvider.settingsConfig).includes(
                "fresh-route-model",
              ),
          ),
      ).toBe(true),
    );

    await waitFor(() =>
      expect(screen.getByText("fresh-route-model")).toBeInTheDocument(),
    );
    expect(
      screen.queryByText("未发现模型目录，保存后可在模型源补充目录"),
    ).not.toBeInTheDocument();
  });

  it("updates the Provider catalog without mutating a stale MultiRouter projection", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "kept-model", ownedBy: null, contextWindow: 128000 },
      { id: "removed-model", ownedBy: null, contextWindow: 64000 },
    ]);
    const provider: Provider = {
      id: "codex-curated-source",
      name: "Curated Source",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://curated.example/v1",
        auth: { OPENAI_API_KEY: "key-curated" },
        modelCatalog: {
          models: [{ model: "kept-model" }],
          spawnAgentModels: ["kept-model", "removed-model"],
        },
      },
    };
    const plan = createDraftRoutingPlan([provider], [provider]);
    const route = normalizeCodexRouteForSave(
      {
        label: provider.name,
        targetProviderId: provider.id,
        match: { models: ["kept-model", "removed-model"] },
      },
      0,
      new Set<string>(),
    );
    const stalePlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        modelCatalog: {
          models: [{ model: "kept-model" }, { model: "removed-model" }],
          spawnAgentModels: ["removed-model"],
        },
        codexRouting: {
          enabled: true,
          defaultRouteId: route.id,
          routes: [route],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, stalePlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: stalePlan.id,
        initialProviderId: stalePlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(providersApi.update).toHaveBeenCalled());
    const savedProvider = vi
      .mocked(providersApi.update)
      .mock.calls.map(([saved]) => saved)
      .find((saved) => saved.id === provider.id)!;
    expect(
      savedProvider.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["kept-model"]);
    expect(savedProvider.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "kept-model",
    ]);

    expect(
      vi
        .mocked(providersApi.update)
        .mock.calls.some(([saved]) => saved.id === stalePlan.id),
    ).toBe(false);
  });

  it("opens the Codex add-source flow when route picker has no model sources", async () => {
    const onCreateProvider = vi.fn();
    const plan = createDraftRoutingPlan([], []);
    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider,
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "编辑匹配规则" }));
    await user.click(screen.getByRole("button", { name: "添加模型源" }));

    expect(onCreateProvider).toHaveBeenCalledTimes(1);
  });

  it("scrolls to the inline route picker after opening route editing", async () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
    const provider: Provider = {
      id: "codex-qwen-scroll",
      name: "Qwen Scroll Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const plan = createDraftRoutingPlan([provider], [provider]);
    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "编辑匹配规则" }));

    await waitFor(() =>
      expect(scrollIntoView).toHaveBeenCalledWith({
        behavior: "smooth",
        block: "nearest",
      }),
    );
  });

  it("resets the workspace scroll position when jumping between multirouter tabs", async () => {
    const scrollTo = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });
    const provider: Provider = {
      id: "codex-scroll-tab-source",
      name: "Scroll Tab Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const plan = createDraftRoutingPlan([provider], [provider]);
    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "overview",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    scrollTo.mockClear();
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "查看链路状态" }));

    await waitFor(() => expect(scrollTo).toHaveBeenCalledWith(0, 0));
  });

  it("keeps a visible alias when route page refreshes provider models from upstream ids", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: null, contextWindow: 272000 },
    ]);
    const provider: Provider = {
      id: "codex-thirdparty-gpt",
      name: "Third-party GPT",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://thirdparty.example/v1",
        auth: { OPENAI_API_KEY: "key-thirdparty" },
        modelCatalog: {
          models: [
            {
              model: "gpt-5.5-thirdparty",
              upstreamModel: "gpt-5.5",
              displayName: "Third-party GPT",
            },
          ],
        },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(
        vi.mocked(providersApi.update).mock.calls.some(([savedProvider]) => {
          if (savedProvider.id !== provider.id) return false;
          return (
            JSON.stringify(savedProvider.settingsConfig.modelCatalog.models) ===
            JSON.stringify([
              {
                model: "gpt-5.5-thirdparty",
                upstreamModel: "gpt-5.5",
                displayName: "Third-party GPT",
                contextWindow: 272000,
              },
            ])
          );
        }),
      ).toBe(true),
    );
  });

  it("uses Volcengine OpenAPI when the routes page refreshes AgentPlan models", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "ark-code-latest", ownedBy: "volcengine", contextWindow: 262144 },
    ]);
    const provider: Provider = {
      id: "codex-volcengine-agentplan",
      name: "火山 AgentPlan",
      category: "custom",
      meta: {
        partnerPromotionKey: "volcengine_agentplan",
        usage_script: {
          enabled: true,
          language: "javascript",
          code: "",
          accessKeyId: "ak-route-refresh",
          secretAccessKey: "sk-route-refresh",
        },
      },
      settingsConfig: {
        baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
        auth: {},
        modelCatalog: { models: [] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "https://ark.cn-beijing.volces.com/api/coding/v3",
      "",
      false,
      undefined,
      undefined,
      {
        action: "ListArkAgentPlanModel",
        accessKeyId: "ak-route-refresh",
        secretAccessKey: "sk-route-refresh",
      },
    );
    await waitFor(() =>
      expect(
        vi.mocked(providersApi.update).mock.calls.some(([savedProvider]) => {
          if (savedProvider.id !== provider.id) return false;
          return JSON.stringify(savedProvider.settingsConfig).includes(
            "ark-code-latest",
          );
        }),
      ).toBe(true),
    );
  });

  it("falls back to data-plane models for AgentPlan routes when AK/SK is missing but API Key exists", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "ark-code-latest", ownedBy: "volcengine", contextWindow: 262144 },
    ]);
    const provider: Provider = {
      id: "codex-volcengine-agentplan-api-key",
      name: "火山 AgentPlan",
      category: "custom",
      meta: {
        partnerPromotionKey: "volcengine_agentplan",
      },
      settingsConfig: {
        baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
        auth: { OPENAI_API_KEY: "sk-volc-route" },
        modelCatalog: { models: [] },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([provider], [provider]),
      provider,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [provider, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "https://ark.cn-beijing.volces.com/api/coding/v3",
      "sk-volc-route",
      false,
      undefined,
      undefined,
      undefined,
    );
    await waitFor(() =>
      expect(
        vi.mocked(providersApi.update).mock.calls.some(([savedProvider]) => {
          if (savedProvider.id !== provider.id) return false;
          return JSON.stringify(savedProvider.settingsConfig).includes(
            "ark-code-latest",
          );
        }),
      ).toBe(true),
    );
  });

  it("does not force the workspace back to routes after the initial jump is consumed", async () => {
    const source: Provider = {
      id: "codex-qwen",
      name: "Qwen Local",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    const providers = [source, plan];
    const props = {
      providers,
      isProxyRunning: true,
      isCodexTakeoverActive: true,
      activeProviderId: plan.id,
      initialProviderId: plan.id,
      initialTab: "routes" as const,
      onEditProvider: vi.fn(),
      onDeletePlan: vi.fn(),
      onCreateProvider: vi.fn(),
    };

    const { rerender } = renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, props),
    );

    expect(screen.getByRole("tab", { name: "路由规则" })).toHaveAttribute(
      "data-state",
      "active",
    );

    const user = userEvent.setup();
    const statusTab = screen.getByRole("tab", { name: "状态" });
    await user.click(statusTab);
    await waitFor(() =>
      expect(statusTab).toHaveAttribute("data-state", "active"),
    );

    rerender(
      React.createElement(
        QueryClientProvider,
        {
          client: new QueryClient({
            defaultOptions: { queries: { retry: false } },
          }),
        },
        React.createElement(CodexRouterWorkspacePage, {
          ...props,
          providers: [...providers],
        }),
      ),
    );

    await waitFor(() =>
      expect(statusTab).toHaveAttribute("data-state", "active"),
    );
  });

  function createSubagentWorkspaceFixture() {
    const source: Provider = {
      id: "codex-deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "deepseek-v4-flash" },
            { model: "deepseek-v4-pro" },
            { model: "qwen3.8" },
          ],
        },
      },
    };
    const draftPlan = withEnabledProviderRoute(
      createDraftRoutingPlan([source], [source]),
      source,
    );
    const plan: Provider = {
      ...draftPlan,
      settingsConfig: {
        ...draftPlan.settingsConfig,
        codexRouting: {
          ...draftPlan.settingsConfig?.codexRouting,
          subagentVersion: "v2",
          spawnAgentModels: ["deepseek-v4-pro", "deepseek-v4-flash"],
          subagentV2: {
            schemaVersion: 1,
            selectionPolicy: "balanced",
            profiles: {
              "deepseek-v4-flash": {
                model: "deepseek-v4-flash",
                enabled: true,
                questionnaire: {
                  taskStrengths: ["repository_exploration"],
                  optimization: "speed",
                  writeScope: "read_only",
                  preference: "preferred",
                  reasoningEffort: "medium",
                },
              },
            },
          },
        },
      },
    };
    return { source, plan };
  }

  function renderSubagentWorkspace(source: Provider, plan: Provider) {
    return renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );
  }

  it("moves Sub-Agent settings into a dedicated top-level workspace tab", async () => {
    const { source, plan } = createSubagentWorkspaceFixture();
    renderSubagentWorkspace(source, plan);

    expect(
      within(screen.getByRole("tablist"))
        .getAllByRole("tab")
        .map((tab) => tab.textContent?.trim()),
    ).toEqual([
      "总览",
      "模型源",
      "路由规则",
      "模型排序",
      "子 Agent",
      "状态",
      "测试发布",
    ]);
    expect(screen.queryByText("Sub-Agent 设置")).not.toBeInTheDocument();

    await userEvent
      .setup()
      .click(screen.getByRole("tab", { name: "子 Agent" }));
    expect(await screen.findByText("Sub-Agent 设置")).toBeInTheDocument();
  });

  it("renders the active protocol as disabled and the inactive protocol as actionable", async () => {
    const { source, plan } = createSubagentWorkspaceFixture();
    const existingRoutes = plan.settingsConfig?.codexRouting?.routes ?? [];
    const existingV2 = plan.settingsConfig?.codexRouting?.subagentV2;
    renderSubagentWorkspace(source, plan);
    const user = userEvent.setup();
    await user.click(screen.getByRole("tab", { name: "子 Agent" }));

    const activeV2 = screen.getByRole("button", { name: "已启用 V2" });
    const inactiveV1 = screen.getByRole("button", { name: "启用 V1" });
    expect(activeV2).toBeDisabled();
    expect(activeV2).toHaveClass("bg-muted");
    expect(inactiveV1).toBeEnabled();
    expect(inactiveV1).toHaveClass("bg-blue-600");
    expect(inactiveV1.closest('[data-subagent-protocol="v1"]')).toHaveClass(
      "border-sky-200",
      "bg-sky-50/70",
      "dark:border-sky-500/40",
      "dark:bg-sky-950/20",
    );
    expect(activeV2.closest('[data-subagent-protocol="v2"]')).toHaveClass(
      "border-emerald-300",
      "bg-emerald-50",
      "dark:border-emerald-500/60",
      "dark:bg-emerald-500/10",
    );

    await user.click(inactiveV1);

    await waitFor(() => expect(providersApi.update).toHaveBeenCalledOnce());
    const [updatedProvider, appType] = vi.mocked(providersApi.update).mock
      .calls[0];
    expect(appType).toBe("codex");
    expect(updatedProvider.settingsConfig?.codexRouting).toMatchObject({
      subagentVersion: "v1",
      routes: existingRoutes,
      subagentV2: existingV2,
    });
    expect(updatedProvider.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(
      updatedProvider.settingsConfig?.codexRouting?.spawnAgentModels,
    ).toEqual(["deepseek-v4-pro", "deepseek-v4-flash"]);
    expect(screen.getByRole("button", { name: "已启用 V1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "启用 V2" })).toBeEnabled();
    expect(
      screen.getByText(/重启 Codex\/app-server 并新建会话后生效/),
    ).toBeInTheDocument();
    expect(screen.getByText(/V1 direct model override/)).toBeInTheDocument();
    expect(screen.getByText("可拖拽排序的前五候选")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "保存排序" }),
    ).toBeInTheDocument();
  });

  it("configures V2 capabilities before selecting and saving the shared advertised model order", async () => {
    const { source, plan } = createSubagentWorkspaceFixture();
    const existingRouting = plan.settingsConfig?.codexRouting;
    renderSubagentWorkspace(source, plan);
    const user = userEvent.setup();

    await user.click(screen.getByRole("tab", { name: "子 Agent" }));

    const configureHeading = screen.getByText(
      "第一步：配置 V2 子 Agent 模型与能力",
    );
    const orderHeading = screen.getByText("第二步：选择 V2 工具说明的前五模型");
    expect(
      configureHeading.compareDocumentPosition(orderHeading) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText("可拖拽排序的前五候选")).toBeInTheDocument();

    expect(
      screen.getByRole("button", { name: "拖动 qwen3.8" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "deepseek-v4-flash 前五 #2" }),
    );
    await user.click(screen.getByRole("button", { name: "保存排序" }));

    await waitFor(() => expect(providersApi.update).toHaveBeenCalledOnce());
    const [savedProvider, appType] = vi.mocked(providersApi.update).mock
      .calls[0];
    expect(appType).toBe("codex");
    expect(savedProvider.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(savedProvider.settingsConfig?.codexRouting).toMatchObject({
      ...existingRouting,
      spawnAgentModels: ["deepseek-v4-pro", "qwen3.8"],
    });
  });

  it("disables protocol actions while switching and keeps the previous protocol after failure", async () => {
    const { source, plan } = createSubagentWorkspaceFixture();
    const switching = createDeferred<boolean>();
    vi.mocked(providersApi.update).mockReturnValueOnce(switching.promise);
    renderSubagentWorkspace(source, plan);
    const user = userEvent.setup();
    await user.click(screen.getByRole("tab", { name: "子 Agent" }));

    await user.click(screen.getByRole("button", { name: "启用 V1" }));
    expect(screen.getByRole("button", { name: "切换中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "已启用 V2" })).toBeDisabled();

    switching.reject(new Error("persist failed"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "切换失败：persist failed",
    );
    expect(screen.getByRole("button", { name: "已启用 V2" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "启用 V1" })).toBeEnabled();
    expect(
      screen.queryByText(/V1 direct model override/),
    ).not.toBeInTheDocument();
  });

  it("shows a missing V2 role model without hiding the routable role", async () => {
    const source: Provider = {
      id: "codex-deepseek-flash-only",
      name: "DeepSeek Flash only",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "deepseek-v4-flash" }],
        },
      },
    };
    const plan = withEnabledProviderRoute(
      createDraftRoutingPlan([source], [source]),
      source,
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await userEvent.click(screen.getByRole("tab", { name: "子 Agent" }));

    expect(screen.getByText("可路由")).toBeInTheDocument();
    expect(screen.getByText("目录中缺失")).toBeInTheDocument();
  });

  it("exposes a delete action for routing plans inside the workspace", async () => {
    const source: Provider = {
      id: "codex-qwen",
      name: "Qwen Local",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    const onDeletePlan = vi.fn();

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan,
        onCreateProvider: vi.fn(),
      }),
    );

    await userEvent
      .setup()
      .click(screen.getAllByRole("button", { name: "删除" })[0]);

    expect(onDeletePlan).toHaveBeenCalledWith(plan);
  });

  it("creates a real routing plan instead of a plain model source", () => {
    const openai: Provider = {
      id: "codex-openai",
      name: "OpenAI",
      category: "official",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "gpt-5.4-mini",
              displayName: "GPT 5.4 Mini",
              contextWindow: 128000,
            },
          ],
        },
      },
      meta: { apiFormat: "openai_responses" },
    };
    const qwen: Provider = {
      id: "codex-qwen",
      name: "Qwen Local",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "qwen3.6",
              displayName: "Qwen 3.6",
              contextWindow: 262144,
            },
          ],
        },
      },
      meta: { apiFormat: "openai_chat" },
    };

    const plan = createDraftRoutingPlan([openai, qwen], [openai, qwen]);

    expect(plan.id).toBe("codex-multirouter");
    expect(isRoutingPlan(plan)).toBe(true);
    expect(plan.settingsConfig.base_url).toBe("http://127.0.0.1:15721/v1");
    expect(plan.settingsConfig.baseUrl).toBe("http://127.0.0.1:15721/v1");
    expect(readCodexRouting(plan)?.enabled).toBe(true);
    expect(readCodexRouting(plan)?.routes).toEqual([]);
    expect(plan.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(plan.settingsConfig.codexRouting).toMatchObject({
      schemaVersion: 2,
      spawnAgentModels: ["gpt-5.4-mini", "qwen3.6"],
    });
  });

  it("preserves catalog upstream models when creating and rebuilding routing plans", () => {
    const thirdParty: Provider = {
      id: "codex-thirdparty-gpt",
      name: "Third-party GPT",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "gpt-5.5-thirdparty",
              upstreamModel: "gpt-5.5",
              displayName: "Third-party GPT",
              contextWindow: 272000,
            },
          ],
        },
      },
      meta: { apiFormat: "openai_responses" },
    };

    const plan = createDraftRoutingPlan([thirdParty], [thirdParty]);
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: thirdParty.name,
          targetProviderId: thirdParty.id,
          match: { models: ["gpt-5.5-thirdparty"], prefixes: [] },
        },
        0,
        new Set<string>(),
      ),
    ];

    expect(plan.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(
      buildModelCatalogForRoutes(
        plan,
        routes,
        new Map([[thirdParty.id, thirdParty]]),
      ).models,
    ).toEqual([
      {
        model: "gpt-5.5-thirdparty",
        upstreamModel: "gpt-5.5",
        displayName: "Third-party GPT",
        contextWindow: 272000,
      },
    ]);
  });

  it("aliases duplicate provider models when saving manual multirouter routes", async () => {
    const official: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
      meta: { apiFormat: "openai_responses" },
    };
    const relay: Provider = {
      id: "codex-relay-gpt",
      name: "Relay GPT",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "gpt-5.5",
              displayName: "Relay GPT 5.5",
              upstreamModel: "gpt-5.5",
            },
          ],
        },
      },
      meta: { apiFormat: "openai_chat" },
    };
    const plan = createDraftRoutingPlan([official, relay], [official, relay]);
    const officialRoute = normalizeCodexRouteForSave(
      {
        label: official.name,
        targetProviderId: official.id,
        match: { models: ["gpt-5.5"], prefixes: ["gpt"] },
        upstream: { apiFormat: "openai_responses" },
      },
      0,
      new Set<string>(),
    );
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          routes: [
            serializeCodexRouteV2(
              { ...officialRoute, modelSelection: { mode: "all" } },
              0,
            ),
          ],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [official, relay, routedPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: routedPlan.id,
        initialProviderId: routedPlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "编辑匹配规则" }));
    await user.click(screen.getByRole("button", { name: "启用" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => expect(providersApi.update).toHaveBeenCalled());
    const updateCalls = vi.mocked(providersApi.update).mock.calls;
    const savedPlan = updateCalls[updateCalls.length - 1]?.[0] as Provider;
    const savedRoutes = readCodexRouting(savedPlan)?.routes ?? [];
    const relayRoute = savedRoutes.find(
      (route) => route.targetProviderId === relay.id,
    );

    expect(savedPlan.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(relayRoute?.modelSelection).toEqual({ mode: "all" });
    expect(relayRoute?.aliases).toEqual({
      "gpt-5.5-relay-gpt": "gpt-5.5",
    });
  });

  it("repairs duplicate exact route models before saving manual routes", () => {
    const official: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
      meta: { apiFormat: "openai_responses" },
    };
    const relay: Provider = {
      id: "codex-relay-gpt",
      name: "Relay GPT",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "gpt-5.5-relay-gpt",
              displayName: "Relay GPT 5.5",
              upstreamModel: "gpt-5.5",
            },
          ],
        },
      },
      meta: { apiFormat: "openai_chat" },
    };
    const plan = createDraftRoutingPlan([official, relay], [official, relay]);
    const usedRouteIds = new Set<string>();
    const badRoutes = [
      normalizeCodexRouteForSave(
        {
          id: "router-official",
          label: official.name,
          targetProviderId: official.id,
          match: { models: ["gpt-5.5"], prefixes: ["gpt"] },
          upstream: { apiFormat: "openai_responses" },
        },
        0,
        usedRouteIds,
      ),
      normalizeCodexRouteForSave(
        {
          id: "router-relay",
          label: relay.name,
          targetProviderId: relay.id,
          match: { models: ["gpt-5.5"], prefixes: ["gpt"] },
          upstream: { apiFormat: "openai_chat" },
        },
        1,
        usedRouteIds,
      ),
    ];

    const normalizedRoutes = normalizeCodexRoutesForVisibleModelAliases(
      plan,
      badRoutes,
      new Map([
        [official.id, official],
        [relay.id, relay],
      ]),
    );
    const catalog = buildModelCatalogForRoutes(
      plan,
      normalizedRoutes,
      new Map([
        [official.id, official],
        [relay.id, relay],
      ]),
    );

    expect(normalizedRoutes[0].match?.models).toEqual(["gpt-5.5"]);
    expect(normalizedRoutes[1].match?.models).toEqual(["gpt-5.5-relay-gpt"]);
    expect(normalizedRoutes[1].upstream?.modelMap).toEqual({
      "gpt-5.5-relay-gpt": "gpt-5.5",
    });
    expect(catalog.models.map((model) => model.model)).toEqual([
      "gpt-5.5",
      "gpt-5.5-relay-gpt",
    ]);
  });

  it("does not expose provider catalog models that no saved route can match", () => {
    const official: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
      meta: { apiFormat: "openai_responses" },
    };
    const longnows: Provider = {
      id: "codex-longnows",
      name: "LongNows",
      category: "custom",
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
      meta: { apiFormat: "openai_chat" },
    };
    const plan = createDraftRoutingPlan(
      [official, longnows],
      [official, longnows],
    );
    const routes = [
      normalizeCodexRouteForSave(
        {
          id: "router-codex-official",
          label: official.name,
          targetProviderId: official.id,
          match: { models: ["gpt-5.5"], prefixes: ["gpt"] },
          upstream: { apiFormat: "openai_responses" },
        },
        0,
        new Set<string>(),
      ),
      normalizeCodexRouteForSave(
        {
          id: "router-longnows-claude",
          label: longnows.name,
          targetProviderId: longnows.id,
          match: { models: ["claude-opus-4-8"], prefixes: ["claude"] },
          upstream: { apiFormat: "openai_chat" },
        },
        1,
        new Set<string>(),
      ),
    ];

    const catalog = buildModelCatalogForRoutes(
      plan,
      routes,
      new Map([
        [official.id, official],
        [longnows.id, longnows],
      ]),
    );

    expect(catalog.models.map((model) => model.model)).toEqual([
      "gpt-5.5",
      "claude-opus-4-8",
    ]);
    expect(
      catalog.models.find((model) => model.model === "gpt-5.5-longnows-gpt"),
    ).toBeUndefined();
  });

  it("reads legacy array codexRouting without clearing routes", () => {
    const plan: Provider = {
      id: "codex-multirouter",
      name: "Codex GPT + DeepSeek 自动路由",
      category: "custom",
      settingsConfig: {
        codexRouting: [
          {
            id: "router-codex-official",
            label: "OpenAI Official",
            providerId: "codex-official",
            models: ["gpt-5.5"],
          },
          {
            id: "router-deepseek",
            label: "DeepSeek",
            providerId: "codex-deepseek",
            modelPrefixes: ["deepseek-"],
          },
        ],
      },
    };

    const routing = readCodexRouting(plan);

    expect(isRoutingPlan(plan)).toBe(true);
    expect(routing?.enabled).toBe(true);
    expect(routing?.routes).toHaveLength(2);
    expect(routing?.routes?.[0].id).toBe("router-codex-official");
    expect(routing?.routes?.[0].targetProviderId).toBe("codex-official");
    expect(routing?.routes?.[0].match?.models).toEqual(["gpt-5.5"]);
    expect(routing?.routes?.[1].match?.prefixes).toEqual(["deepseek-"]);
  });

  it("keeps legacy provider references and dedupes equivalent route candidates", () => {
    const deepseek: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "deepseek-v4-flash" }],
        },
      },
    };
    const plan: Provider = {
      id: "codex-multirouter",
      name: "Legacy Router",
      category: "custom",
      settingsConfig: {
        codexRouting: [
          {
            id: "legacy-deepseek",
            label: "DeepSeek",
            provider: "deepseek",
            models: ["deepseek-v4-flash"],
          },
          {
            id: "router-deepseek",
            label: "DeepSeek",
            targetProviderId: "deepseek",
            match: { models: ["deepseek-v4-flash"] },
          },
        ],
      },
    };

    const routes = readCodexRouting(plan)?.routes ?? [];
    const deduped = dedupeCodexRoutesBySemanticProvider(routes, [deepseek]);

    expect(routes[0].targetProviderId).toBe("deepseek");
    expect(deduped).toHaveLength(1);
    expect(deduped[0].id).toBe("legacy-deepseek");
  });

  it("normalizes selected router candidates into visible routes and catalog models", () => {
    const qwen: Provider = {
      id: "codex-qwen",
      name: "Qwen Local",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "qwen3.6",
              displayName: "Qwen 3.6",
              contextWindow: 262144,
            },
          ],
        },
      },
      meta: { apiFormat: "openai_chat" },
    };
    const deepseek: Provider = {
      id: "codex-deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "deepseek-v4-flash",
              contextWindow: 1000000,
              inputModalities: ["text"],
              textOnly: true,
              supportsImage: false,
            },
          ],
        },
      },
      meta: { apiFormat: "openai_chat" },
    };
    const plan = createDraftRoutingPlan([qwen, deepseek], [qwen, deepseek]);
    const usedRouteIds = new Set<string>();
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: "Qwen Local",
          targetProviderId: qwen.id,
          match: { models: ["qwen3.6"], prefixes: ["qwen"] },
          upstream: { apiFormat: "openai_chat" },
        },
        0,
        usedRouteIds,
      ),
      normalizeCodexRouteForSave(
        {
          label: "DeepSeek",
          targetProviderId: deepseek.id,
          match: { models: ["deepseek-v4-flash"], prefixes: ["deepseek"] },
          upstream: { apiFormat: "openai_chat" },
        },
        1,
        usedRouteIds,
      ),
    ];
    const savedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        modelCatalog: buildModelCatalogForRoutes(
          plan,
          routes,
          new Map([
            [qwen.id, qwen],
            [deepseek.id, deepseek],
          ]),
        ),
        codexRouting: {
          enabled: true,
          defaultRouteId: routes[0].id,
          routes,
        },
      },
    };

    expect(isRoutingPlan(savedPlan)).toBe(true);
    expect(readCodexRouting(savedPlan)?.routes).toHaveLength(2);
    expect(
      (readCodexRouting(savedPlan)?.routes ?? []).map((route) => route.id),
    ).toEqual(["codex-qwen", "codex-deepseek"]);
    expect(savedPlan.settingsConfig.modelCatalog.models).toEqual([
      { model: "qwen3.6", displayName: "Qwen 3.6", contextWindow: 262144 },
      {
        model: "deepseek-v4-flash",
        contextWindow: 1000000,
        inputModalities: ["text"],
        textOnly: true,
        supportsImage: false,
        capabilities: { inputModalities: ["text"], textOnly: true },
      },
    ]);
    expect(savedPlan.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "qwen3.6",
      "deepseek-v4-flash",
    ]);
  });

  it("adds and enables a new provider candidate without using select-all", async () => {
    const openai: Provider = {
      id: "codex-openai",
      name: "OpenAI Official",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    };
    const qwen: Provider = {
      id: "codex-qwen-local",
      name: "Qwen Local vLLM",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const plan = createDraftRoutingPlan([openai, qwen], [openai, qwen]);
    const openaiRoute = normalizeCodexRouteForSave(
      {
        label: openai.name,
        targetProviderId: openai.id,
        match: { models: ["gpt-5.5"], prefixes: ["gpt"] },
      },
      0,
      new Set<string>(),
    );
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          routes: [
            serializeCodexRouteV2(
              { ...openaiRoute, modelSelection: { mode: "all" } },
              0,
            ),
          ],
        },
      },
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [openai, qwen, routedPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: routedPlan.id,
        initialProviderId: routedPlan.id,
        initialTab: "routes",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "编辑匹配规则" }));
    await user.click(screen.getByRole("button", { name: "启用" }));
    await user.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => expect(providersApi.update).toHaveBeenCalled());
    const updateCalls = vi.mocked(providersApi.update).mock.calls;
    const savedPlan = updateCalls[updateCalls.length - 1]?.[0];
    const savedRoutes = readCodexRouting(savedPlan as Provider)?.routes ?? [];
    const qwenRoute = savedRoutes.find(
      (route) => route.targetProviderId === qwen.id,
    );

    expect(savedRoutes).toHaveLength(2);
    expect(qwenRoute?.enabled).toBe(true);
    expect(qwenRoute?.modelSelection).toEqual({ mode: "all" });
  });

  it("rebuilds route catalog from current targets instead of keeping stale fallback models", () => {
    const qwen: Provider = {
      id: "codex-qwen-local",
      name: "Qwen Local vLLM",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "qwen3.6",
              displayName: "Qwen 3.6",
              contextWindow: 262144,
            },
          ],
        },
      },
    };
    const plan = createDraftRoutingPlan([], []);
    const stalePlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        modelCatalog: {
          models: [
            { model: "gpt-5.5" },
            { model: "gpt-5.4" },
            { model: "gpt-5.4-mini" },
            { model: "gpt-5.3-codex-spark" },
          ],
          spawnAgentModels: ["gpt-5.5", "gpt-5.4"],
        },
      },
    };
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: qwen.name,
          targetProviderId: qwen.id,
          match: { models: ["qwen3.6"], prefixes: ["qwen"] },
        },
        0,
        new Set<string>(),
      ),
    ];

    const rebuilt = buildModelCatalogForRoutes(
      stalePlan,
      routes,
      new Map([[qwen.id, qwen]]),
    );

    expect(rebuilt.models).toEqual([
      {
        model: "qwen3.6",
        displayName: "Qwen 3.6",
        contextWindow: 262144,
      },
    ]);
    expect(rebuilt.spawnAgentModels).toEqual(["qwen3.6"]);
  });

  it("keeps OpenAI/Codex providers empty until their OAuth catalog is fetched", () => {
    const officialBackup: Provider = {
      id: "codex-official-backup",
      name: "OpenAI Official Backup",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    };

    const plan = createDraftRoutingPlan([officialBackup], [officialBackup]);

    expect(plan.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(plan.settingsConfig.codexRouting.spawnAgentModels ?? []).toEqual([]);
  });

  it("rebuilds an official route from the dynamically persisted OAuth catalog", () => {
    const officialBackup: Provider = {
      id: "codex-official-backup",
      name: "OpenAI Official Backup",
      category: "official",
      settingsConfig: {
        auth: {},
        config: "",
        modelCatalog: {
          models: [{ model: "gpt-5.6-sol", contextWindow: 372000 }],
        },
      },
    };
    const plan = createDraftRoutingPlan([officialBackup], [officialBackup]);
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: officialBackup.name,
          targetProviderId: officialBackup.id,
          match: { models: ["gpt-5.6-sol"], prefixes: ["gpt-"] },
        },
        0,
        new Set<string>(),
      ),
    ];

    const rebuilt = buildModelCatalogForRoutes(
      plan,
      routes,
      new Map([[officialBackup.id, officialBackup]]),
    );

    expect(rebuilt.models).toContainEqual({
      model: "gpt-5.6-sol",
      contextWindow: 372000,
    });
  });

  it("excludes disabled routes from the injected model catalog", () => {
    const enabled: Provider = {
      id: "qwen-enabled",
      name: "Qwen Enabled",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const disabled: Provider = {
      id: "openai-disabled",
      name: "OpenAI Disabled",
      category: "official",
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    };
    const plan = createDraftRoutingPlan(
      [enabled, disabled],
      [enabled, disabled],
    );

    const rebuilt = buildModelCatalogForRoutes(
      plan,
      [
        {
          id: "qwen-route",
          enabled: true,
          targetProviderId: enabled.id,
          match: { models: ["qwen3.6"], prefixes: [] },
          upstream: { apiFormat: "openai_responses" },
        },
        {
          id: "openai-route",
          enabled: false,
          targetProviderId: disabled.id,
          match: { models: ["gpt-5.5"], prefixes: ["gpt-"] },
          upstream: { apiFormat: "openai_responses" },
        },
      ],
      new Map([
        [enabled.id, enabled],
        [disabled.id, disabled],
      ]),
    );

    expect(rebuilt.models.map((model) => model.model)).toEqual(["qwen3.6"]);
  });

  it("preserves the full picker order when routes rebuild the model catalog", () => {
    const source: Provider = {
      id: "sorted-source",
      name: "Sorted Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "model-a" }, { model: "model-b" }],
        },
      },
    };
    const plan: Provider = {
      ...createDraftRoutingPlan([source], [source]),
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "model-a", sortIndex: 1 },
            { model: "model-b", sortIndex: 0 },
          ],
          spawnAgentModels: ["model-a"],
        },
      },
    };

    const rebuilt = buildModelCatalogForRoutes(
      plan,
      [
        {
          id: "sorted-route",
          enabled: true,
          targetProviderId: source.id,
          match: { models: ["model-a", "model-b"], prefixes: [] },
          upstream: { apiFormat: "openai_responses" },
        },
      ],
      new Map([[source.id, source]]),
    );

    expect(rebuilt.models).toEqual([
      { model: "model-a", sortIndex: 1 },
      { model: "model-b", sortIndex: 0 },
    ]);
    expect(rebuilt.spawnAgentModels).toEqual(["model-a", "model-b"]);
  });

  it("keeps unsaved route picker enabled draft state across candidate refreshes", () => {
    const currentEnabledIds = new Set(["openai-route"]);

    expect(
      Array.from(
        mergeRoutePickerDraftIds(
          currentEnabledIds,
          ["openai-route", "qwen-route"],
          ["openai-route", "qwen-route"],
          ["qwen-route"],
        ),
      ),
    ).toEqual(["openai-route"]);
  });

  it("applies route picker defaults only to newly discovered candidates", () => {
    const currentEnabledIds = new Set(["openai-route"]);

    expect(
      Array.from(
        mergeRoutePickerDraftIds(
          currentEnabledIds,
          ["openai-route", "qwen-route"],
          ["openai-route", "qwen-route", "deepseek-route"],
          ["qwen-route", "deepseek-route"],
        ),
      ),
    ).toEqual(["openai-route", "deepseek-route"]);
  });

  it("updates multirouter settings without dropping routes or model catalog", () => {
    const qwen: Provider = {
      id: "codex-qwen",
      name: "Qwen Local",
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    };
    const plan = createDraftRoutingPlan([qwen], [qwen]);
    const savedPlan: Provider = {
      ...plan,
      name: "Old MultiRouter",
      notes: "old notes",
      settingsConfig: {
        ...plan.settingsConfig,
        modelCatalog: {
          models: [{ model: "qwen3.6" }],
          spawnAgentModels: ["qwen3.6"],
        },
        codexRouting: {
          enabled: true,
          defaultRouteId: "codex-qwen",
          routes: [
            {
              id: "codex-qwen",
              label: "Qwen Local",
              enabled: true,
              targetProviderId: qwen.id,
              match: { models: ["qwen3.6"] },
            },
          ],
        },
      },
    };

    const updated = applyMultiRouterSettingsDraft(savedPlan, {
      name: "Daily MultiRouter",
      notes: "primary plan",
      enabled: false,
      defaultRouteId: "missing-route",
      officialAuth: { mode: "desktop_current_login" },
      hostedTools: {
        webSearch: false,
        imageGeneration: true,
      },
    });

    expect(updated.name).toBe("Daily MultiRouter");
    expect(updated.notes).toBe("primary plan");
    expect(updated.settingsConfig.base_url).toBe("http://127.0.0.1:15721/v1");
    expect(updated.settingsConfig.baseUrl).toBe("http://127.0.0.1:15721/v1");
    expect(updated.settingsConfig.modelCatalog).toEqual(
      savedPlan.settingsConfig.modelCatalog,
    );
    expect(readCodexRouting(updated)?.enabled).toBe(false);
    expect(readCodexRouting(updated)?.routes).toEqual(
      readCodexRouting(savedPlan)?.routes,
    );
    expect(readCodexRouting(updated)?.defaultRouteId).toBeUndefined();
    expect(updated.settingsConfig.hostedTools).toEqual({
      webSearch: { enabled: false },
      imageGeneration: { enabled: true },
    });
  });

  it("normalizes listener config into a usable Codex proxy base url", () => {
    expect(buildCodexProxyBaseUrl("0.0.0.0", 15721)).toBe(
      "http://127.0.0.1:15721/v1",
    );
    expect(buildCodexProxyBaseUrl("::", 15721)).toBe("http://[::1]:15721/v1");

    expect(validateProxyListenDraft("127.0.0.1", "15721")).toEqual({
      ok: true,
      listenAddress: "127.0.0.1",
      listenPort: 15721,
      baseUrl: "http://127.0.0.1:15721/v1",
    });
    expect(validateProxyListenDraft("127.0.0.1", "abc")).toEqual({
      ok: false,
      error: "监听端口必须是 1024-65535 之间的数字。",
    });
  });

  it("changes only official routes when a legacy Router adopts an auth policy", () => {
    const plan: Provider = {
      id: "legacy-router",
      name: "Legacy Router",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "official",
              targetProviderId: "codex-official",
              match: { models: ["gpt-5.6"] },
              upstream: {
                apiFormat: "openai_responses",
                auth: {
                  source: "managed_codex_oauth",
                  authProvider: "codex_oauth",
                  accountId: "acct-old",
                },
              },
            },
            {
              id: "qwen",
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
    };

    const updated = applyMultiRouterSettingsDraft(plan, {
      name: plan.name,
      enabled: true,
      officialAuth: { mode: "account_pool" },
      hostedTools: {
        webSearch: true,
        imageGeneration: false,
      },
    });
    const routing = readCodexRouting(updated)!;

    expect(routing.officialAuth).toEqual({ mode: "account_pool" });
    expect(routing.routes?.[0].upstream?.auth).toEqual({
      source: "account_pool",
    });
    expect(routing.routes?.[1].upstream?.auth).toEqual({
      source: "provider_config",
    });
  });

  it("reports multirouter runtime state from current provider and takeover status", () => {
    const plan = createDraftRoutingPlan([], []);

    expect(
      buildMultiRouterRuntimeStatus({
        selectedPlan: plan,
        selectedRouting: readCodexRouting(plan),
        enabledRouteCount: 1,
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: "other-router",
      }).label,
    ).toBe("未发布");

    expect(
      buildMultiRouterRuntimeStatus({
        selectedPlan: plan,
        selectedRouting: readCodexRouting(plan),
        enabledRouteCount: 1,
        isProxyRunning: false,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
      }).label,
    ).toBe("代理未启动");

    expect(
      buildMultiRouterRuntimeStatus({
        selectedPlan: plan,
        selectedRouting: readCodexRouting(plan),
        enabledRouteCount: 0,
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
      }).label,
    ).toBe("无启用规则");

    expect(
      buildMultiRouterRuntimeStatus({
        selectedPlan: plan,
        selectedRouting: readCodexRouting(plan),
        enabledRouteCount: 1,
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
      }),
    ).toMatchObject({
      running: true,
      label: "运行中",
      tone: "ok",
    });
  });

  // 当本地代理、Codex 接管、当前方案路由和最新真实转发都成功时，
  // 状态页应在原地给出完成结果，不再触发 App 进入历史修复。
  it("shows runtime validation success in the workspace without a post-setup callback", async () => {
    const source: Provider = {
      id: "codex-online-source",
      name: "Online Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "test-model" }],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: source.name,
          targetProviderId: source.id,
          match: { models: ["test-model"] },
        },
        0,
        new Set<string>(),
      ),
    ];
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          enabled: true,
          defaultRouteId: routes[0].id,
          routes,
        },
      },
    };
    requestLogsFixture.value = {
      data: {
        data: [
          createCodexProxyLog({
            providerId: "codex-online-source",
            model: "test-model",
            requestModel: "test-model",
          }),
        ],
        total: 1,
        page: 0,
        pageSize: 500,
      },
      isLoading: false,
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, routedPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: routedPlan.id,
        initialProviderId: routedPlan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByText("MultiRouter 已通过真实请求验证"),
      ).toBeInTheDocument(),
    );
  });

  it("shows stale projection details and lets the user resync with readable provider names", async () => {
    const source: Provider = {
      id: "5626e6b9-33cb-4c3b-8d16-af8176e16209",
      name: "DeepSeek Relay",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            {
              model: "deepseek-v4-flash",
              upstreamModel: "deepseek-v4-flash-0731",
            },
          ],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    const staleStatus: CodexRoutingProjectionStatus = {
      schemaVersion: 1,
      routerProviderId: plan.id,
      state: "pending",
      dependencyFingerprint: "stale-fingerprint",
      generatedAt: "2026-08-21T00:00:00Z",
      warnings: [],
      routes: [
        {
          routeId: `router-${source.id}`,
          routeLabel: `router-${source.id}`,
          targetProviderId: source.id,
          targetProviderName: source.name,
          visibleModel: "deepseek-v4-flash",
          canonicalModel: "deepseek-v4-flash",
          upstreamModel: "deepseek-v4-flash-0731",
          apiFormat: "openai_responses",
          apiFormatSource: "provider",
          authOwner: "provider_config",
          capabilitySources: {
            contextWindow: "provider_model",
            inputModalities: "provider_model",
            reasoning: "provider_model",
            codexCache: "provider_model",
          },
        },
      ],
      lastErrorCode: "projection_stale",
      lastError:
        "Codex MultiRouter projection dependencies changed and regeneration is required",
    };
    vi.mocked(
      providersApi.inspectCodexMultiRouterProjection,
    ).mockResolvedValueOnce(staleStatus);
    vi.mocked(
      providersApi.retryCodexMultiRouterProjection,
    ).mockResolvedValueOnce({
      ...staleStatus,
      state: "ready",
      dependencyFingerprint: "fresh-fingerprint",
      lastErrorCode: null,
      lastError: null,
    });

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByText("MultiRouter 目录投影：待同步"),
      ).toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(screen.getByText("当前有效映射")).toBeInTheDocument(),
    );
    expect(
      screen.getByText(/DeepSeek Relay \/ DeepSeek Relay/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/deepseek-v4-flash → deepseek-v4-flash-0731/),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(`router-${source.id}`, { exact: true }),
    ).not.toBeInTheDocument();

    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "重新同步目录" }));

    await waitFor(() =>
      expect(providersApi.retryCodexMultiRouterProjection).toHaveBeenCalledWith(
        plan.id,
      ),
    );
    await waitFor(() =>
      expect(
        screen.getByText("MultiRouter 目录投影：已同步"),
      ).toBeInTheDocument(),
    );
  });

  it("shows a visible result after refreshing multirouter validation state", async () => {
    const source: Provider = {
      id: "codex-refresh-source",
      name: "Refresh Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "refresh-model" }],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "刷新校验" }));

    await waitFor(() =>
      expect(
        screen.getByText("已刷新校验状态，请查看链路卡片和最近转发表。"),
      ).toBeInTheDocument(),
    );
  });

  // 解锁模型菜单是 issue #10 的关键恢复动作，必须同时固定提示文案和真实 API 调用。
  it("explains and triggers the Codex model picker unlock action", async () => {
    const source: Provider = {
      id: "codex-unlock-source",
      name: "Unlock Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "unlock-model" }],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    vi.mocked(proxyApi.unlockCodexModelPicker).mockResolvedValueOnce({
      attemptedPorts: [9222],
      debugPort: 9222,
      targetId: "target-1",
      targetTitle: "Codex",
      targetUrl: "app://codex",
      modelCount: 1,
      modelNames: ["unlock-model"],
      injected: true,
      launched: false,
      codexExecutable: null,
      message: "已注入模型菜单白名单",
    });

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    expect(
      screen.getByText(
        "开启或确认 Codex 接管后会自动尝试一次；若当前 Desktop 已普通启动且菜单仍只显示“自定义”，请完全退出 Codex Desktop 后点击“解锁模型菜单”。CLI/app-server 的模型目录修复走 live config、model_catalog_json 和本地 /v1/models，不需要把小写 codex.exe 当 Desktop 启动。",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByTitle(
        /CLI\/app-server 仍由 config\.toml、model_catalog_json、本地 \/v1\/models 和 MultiRouter 路由支持/,
      ),
    ).toBeInTheDocument();

    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: "解锁模型菜单" }));

    await waitFor(() =>
      expect(proxyApi.unlockCodexModelPicker).toHaveBeenCalledTimes(1),
    );
    expect(proxyApi.unlockCodexModelPicker).toHaveBeenCalledWith();
    expect(screen.getByText("模型菜单白名单已注入")).toBeInTheDocument();
    expect(
      screen.queryByText(
        "开启或确认 Codex 接管后会自动尝试一次；若当前 Desktop 已普通启动且菜单仍只显示“自定义”，请完全退出 Codex Desktop 后点击“解锁模型菜单”。CLI/app-server 的模型目录修复走 live config、model_catalog_json 和本地 /v1/models，不需要把小写 codex.exe 当 Desktop 启动。",
      ),
    ).not.toBeInTheDocument();
  });

  // Desktop 主程序发现属于后端自动探测；前端只展示诊断路径，不记忆也不回传 exe。
  it("keeps Codex Desktop executable discovery owned by the backend", async () => {
    const source: Provider = {
      id: "codex-desktop-retry-source",
      name: "Desktop Retry Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "desktop-model" }],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    const codexExecutable =
      "C:\\Program Files\\WindowsApps\\OpenAI.Codex_26.623.141536.0_x64__2p2nqsd0c76g0\\app\\Codex.exe";
    vi.mocked(proxyApi.unlockCodexModelPicker)
      .mockResolvedValueOnce({
        attemptedPorts: [9229],
        debugPort: null,
        targetId: null,
        targetTitle: null,
        targetUrl: null,
        modelCount: 1,
        modelNames: ["desktop-model"],
        injected: false,
        launched: false,
        codexExecutable,
        message:
          "Codex Desktop is already running without an injectable CDP port.",
      })
      .mockResolvedValueOnce({
        attemptedPorts: [9229],
        debugPort: 9229,
        targetId: "desktop-target",
        targetTitle: "Codex",
        targetUrl: "app://codex",
        modelCount: 1,
        modelNames: ["desktop-model"],
        injected: true,
        launched: true,
        codexExecutable,
        message: "Codex Desktop model picker whitelist patch was injected.",
      });

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "解锁模型菜单" }));

    await waitFor(() =>
      expect(screen.getByText("模型菜单白名单尚未注入")).toBeInTheDocument(),
    );
    expect(screen.getByText(/Codex Desktop 主程序/)).toBeInTheDocument();
    expect(
      screen.getByText(
        /已捕获该 Desktop 路径；请完全退出 Codex Desktop 后再次点击“解锁模型菜单”/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/切换第三方 API Key 不需要重复解锁/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/CLI\/app-server 继续使用 live config/),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "解锁模型菜单" }));

    await waitFor(() =>
      expect(proxyApi.unlockCodexModelPicker).toHaveBeenCalledTimes(2),
    );
    expect(proxyApi.unlockCodexModelPicker).toHaveBeenNthCalledWith(1);
    expect(proxyApi.unlockCodexModelPicker).toHaveBeenNthCalledWith(2);
    expect(screen.getByText("模型菜单白名单已注入")).toBeInTheDocument();
  });

  // 找不到 Desktop 时只展示失败原因，避免把 CCSwitchMulti portable 误解成 Codex Desktop portable。
  it("shows unlock failures without offering a manual Codex.exe picker", async () => {
    const source: Provider = {
      id: "codex-desktop-missing-source",
      name: "Desktop Missing Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "missing-desktop-model" }],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    vi.mocked(proxyApi.unlockCodexModelPicker).mockRejectedValueOnce(
      new Error(
        "Codex Desktop executable was not found. Install the Codex Windows app from Microsoft Store.",
      ),
    );

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, plan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: plan.id,
        initialProviderId: plan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "解锁模型菜单" }));

    await waitFor(() =>
      expect(screen.getByText(/模型菜单解锁失败/)).toBeInTheDocument(),
    );
    expect(proxyApi.unlockCodexModelPicker).toHaveBeenCalledWith();
    expect(
      screen.queryByRole("button", { name: "选择 Codex.exe 后解锁" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/ccswitch\.codexDesktopExecutablePath/),
    ).not.toBeInTheDocument();
  });

  // 即使最近一条 Codex 代理日志成功，只要它没有命中当前 MultiRouter 方案的 route，
  // 就不能显示当前方案已通过真实请求验证。
  it("does not show runtime validation success when the latest request is outside the selected route", async () => {
    const source: Provider = {
      id: "codex-online-source",
      name: "Online Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "test-model" }],
        },
      },
    };
    const unrelatedSource: Provider = {
      id: "codex-unrelated-source",
      name: "Unrelated Source",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "other-model" }],
        },
      },
    };
    const plan = createDraftRoutingPlan([source], [source]);
    const routes = [
      normalizeCodexRouteForSave(
        {
          label: source.name,
          targetProviderId: source.id,
          match: { models: ["test-model"] },
        },
        0,
        new Set<string>(),
      ),
    ];
    const routedPlan: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        codexRouting: {
          enabled: true,
          defaultRouteId: routes[0].id,
          routes,
        },
      },
    };
    requestLogsFixture.value = {
      data: {
        data: [
          createCodexProxyLog({
            providerId: unrelatedSource.id,
            providerName: unrelatedSource.name,
            model: "other-model",
            requestModel: "other-model",
            statusCode: 200,
          }),
        ],
        total: 1,
        page: 0,
        pageSize: 500,
      },
      isLoading: false,
    };

    renderWorkspace(
      React.createElement(CodexRouterWorkspacePage, {
        providers: [source, unrelatedSource, routedPlan],
        isProxyRunning: true,
        isCodexTakeoverActive: true,
        activeProviderId: routedPlan.id,
        initialProviderId: routedPlan.id,
        initialTab: "status",
        onEditProvider: vi.fn(),
        onDeletePlan: vi.fn(),
        onCreateProvider: vi.fn(),
      }),
    );

    await waitFor(() =>
      expect(screen.getByText("成功 200")).toBeInTheDocument(),
    );
    expect(screen.queryByText("当前方案成功")).not.toBeInTheDocument();
    expect(
      screen.queryByText("MultiRouter 已通过真实请求验证"),
    ).not.toBeInTheDocument();
  });
});
