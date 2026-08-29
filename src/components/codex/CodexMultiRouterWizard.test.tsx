import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { providersApi } from "@/lib/api/providers";
import { CodexMultiRouterWizard } from "./CodexMultiRouterWizard";

const {
  preflightCodexProviderProtocolCompatibility,
  prepareCodexProviderSetBatch,
  commitCodexProviderSetBatch,
} = vi.hoisted(() => ({
  preflightCodexProviderProtocolCompatibility: vi.fn(),
  prepareCodexProviderSetBatch: vi.fn(),
  commitCodexProviderSetBatch: vi.fn(),
}));

vi.mock("@/lib/api/protocol-compatibility", () => ({
  preflightCodexProviderProtocolCompatibility,
  prepareCodexProviderSetBatch,
  commitCodexProviderSetBatch,
}));

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: () => ({
    accounts: [],
    hasAnyAccount: false,
    isLoadingStatus: false,
  }),
}));

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getCodexMultiRouterRevision: vi.fn().mockResolvedValue("revision-1"),
    previewCodexMultiRouterMigration: vi.fn().mockResolvedValue({
      schemaVersion: 2,
      providerId: "router-b",
      expectedRevision: "revision-1",
      planToken: "opaque-token",
      diff: {
        removedRouteFields: ["upstream.apiFormat"],
        createdProviderIds: [],
        changedRouteIds: ["router-b-route"],
      },
      warnings: [],
      generatedProviders: [],
    }),
    applyCodexMultiRouterMigration: vi.fn(),
    getAll: vi.fn(),
    update: vi.fn(),
    add: vi.fn(),
  },
}));

function renderWizard(
  providers: Provider[],
  options?: { mode?: "create" | "edit"; planId?: string },
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <CodexMultiRouterWizard
        open
        providers={providers}
        mode={options?.mode ?? "create"}
        planId={options?.planId}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("CodexMultiRouterWizard", () => {
  it("keeps V1 and V2 configuration out of the four-stage routing wizard", () => {
    renderWizard([
      {
        id: "codex-deepseek",
        name: "DeepSeek",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [
              { model: "deepseek-v4-flash" },
              { model: "deepseek-v4-pro" },
            ],
          },
        },
      },
    ]);

    expect(
      screen.queryByRole("button", { name: /Sub-Agent V1/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Sub-Agent V2/ }),
    ).not.toBeInTheDocument();
  });

  it("presents MultiRouter setup as four user tasks", () => {
    renderWizard([
      {
        id: "codex-deepseek",
        name: "DeepSeek",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [{ model: "deepseek-v4-flash" }],
          },
        },
      },
    ]);

    expect(
      screen.getByRole("button", { name: "选择模型源" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "自动准备与验证" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "选择模型并预览路由" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "启用并验证" }),
    ).toBeInTheDocument();

    expect(
      screen.queryByRole("button", { name: "理解 MultiRouter" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "获取模型列表" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "处理重名模型" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "保存并发布" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the final save entry visible when no model source exists", () => {
    renderWizard([]);

    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));

    expect(screen.getByRole("button", { name: "保存并发布" })).toBeVisible();
    expect(screen.getByText(/尚未选择模型源，保存入口仍保留/)).toBeVisible();
  });

  it("coalesces rapid saves and updates the same plan after the first save", async () => {
    let resolveCommit:
      | ((value: {
          preview: Record<string, unknown>;
          router: Provider;
          projections: never[];
          status: "committed";
          projectionErrorCode: null;
        }) => void)
      | undefined;
    const firstCommit = new Promise<{
      preview: Record<string, unknown>;
      router: Provider;
      projections: never[];
      status: "committed";
      projectionErrorCode: null;
    }>((resolve) => {
      resolveCommit = resolve;
    });
    prepareCodexProviderSetBatch.mockImplementation(
      async (_sources: unknown, router: Provider) => ({
        digest: `digest:${router.id}`,
        sourcePreviews: [],
        routerProviderId: router.id,
        requiresSplitConfirmation: false,
        blocked: false,
      }),
    );
    commitCodexProviderSetBatch.mockImplementationOnce(() => firstCommit);
    commitCodexProviderSetBatch.mockImplementation(
      async (_sources: unknown, router: Provider, digest: string) => ({
        preview: {
          digest,
          sourcePreviews: [],
          routerProviderId: router.id,
          requiresSplitConfirmation: false,
          blocked: false,
        },
        router,
        projections: [],
        status: "committed" as const,
        projectionErrorCode: null,
      }),
    );
    const source: Provider = {
      id: "relay",
      name: "Relay",
      category: "custom",
      meta: {
        codexProtocolMode: "manual",
        apiFormat: "openai_responses",
      },
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: "relay-model" }] },
      },
    };

    renderWizard([source]);
    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));
    const saveButton = screen.getByRole("button", { name: "保存并发布" });

    fireEvent.click(saveButton);
    fireEvent.click(saveButton);
    await waitFor(() =>
      expect(commitCodexProviderSetBatch).toHaveBeenCalledTimes(1),
    );

    const firstRouter = commitCodexProviderSetBatch.mock
      .calls[0]?.[1] as Provider;
    resolveCommit?.({
      preview: {
        digest: `digest:${firstRouter.id}`,
        sourcePreviews: [],
        routerProviderId: firstRouter.id,
        requiresSplitConfirmation: false,
        blocked: false,
      },
      router: firstRouter,
      projections: [],
      status: "committed",
      projectionErrorCode: null,
    });
    await waitFor(() =>
      expect(screen.getByText("MultiRouter provider 已保存。")).toBeVisible(),
    );

    fireEvent.click(screen.getByRole("button", { name: "保存并发布" }));
    await waitFor(() =>
      expect(commitCodexProviderSetBatch).toHaveBeenCalledTimes(2),
    );
    const secondRouter = commitCodexProviderSetBatch.mock
      .calls[1]?.[1] as Provider;
    expect(secondRouter.id).toBe(firstRouter.id);
    expect(providersApi.add).not.toHaveBeenCalled();
    expect(providersApi.update).not.toHaveBeenCalled();
  });

  it("does not require users to choose subagent models in the main wizard", () => {
    renderWizard([
      {
        id: "codex-deepseek",
        name: "DeepSeek",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [
              { model: "deepseek-v4-flash" },
              { model: "deepseek-v4-pro" },
            ],
          },
        },
      },
    ]);

    expect(screen.queryByText("子 Agent 候选")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/选择并排序最多 5 个子 Agent 候选模型/),
    ).not.toBeInTheDocument();
  });

  it("edits the explicitly selected plan instead of the first cached routing plan", () => {
    const routingPlan = (id: string, name: string): Provider => ({
      id,
      name,
      category: "custom",
      settingsConfig: {
        codexRouting: { enabled: true, routes: [{ id: `${id}-route` }] },
        modelCatalog: { models: [{ model: `${id}-model` }] },
      },
    });

    renderWizard(
      [
        routingPlan("router-a", "旧方案 A"),
        routingPlan("router-b", "目标方案 B"),
      ],
      { mode: "edit", planId: "router-b" },
    );

    expect(screen.getByText("正在编辑：目标方案 B")).toBeVisible();
    expect(screen.getByText("router-b")).toBeVisible();
    expect(screen.queryByText("正在编辑：旧方案 A")).not.toBeInTheDocument();
  });

  it("selects only the Providers referenced by an existing schema-v2 plan", () => {
    const source = (id: string, name: string): Provider => ({
      id,
      name,
      category: "custom",
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: `${id}-model` }] },
      },
    });
    const used = source("used-source", "Used source");
    const unused = source("unused-source", "Unused source");
    const plan: Provider = {
      id: "router-v2",
      name: "Router V2",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          routes: [
            {
              id: "used-route",
              targetProviderId: used.id,
              modelSelection: { mode: "all" },
              authPolicy: { source: "provider_config" },
            },
          ],
        },
      },
    };

    renderWizard([used, unused, plan], { mode: "edit", planId: plan.id });

    expect(screen.getByText(/已选择 1 \/ 2/)).toBeVisible();
    expect(
      screen.getByRole("checkbox", {
        name: "使用 Used source 作为模型源",
      }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", {
        name: "使用 Unused source 作为模型源",
      }),
    ).not.toBeChecked();
  });

  it("requires an explicit redacted migration preview before editing a v1 plan", async () => {
    const legacyPlan: Provider = {
      id: "legacy-plan",
      name: "Legacy Plan",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "must-not-render" },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "legacy-route",
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

    renderWizard([legacyPlan], { mode: "edit", planId: legacyPlan.id });

    expect(
      await screen.findByRole("heading", {
        name: "编辑前迁移旧 MultiRouter",
      }),
    ).toBeVisible();
    expect(providersApi.getCodexMultiRouterRevision).toHaveBeenCalledWith(
      legacyPlan.id,
    );
    expect(screen.queryByText("legacy-secret")).not.toBeInTheDocument();
    expect(screen.queryByText("must-not-render")).not.toBeInTheDocument();
  });

  it("keeps provider-owned protocol and hosted-tool controls out of source selection", () => {
    renderWizard([
      {
        id: "third-party",
        name: "Third party source",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: { models: [{ model: "third-party-model" }] },
        },
      },
    ]);

    expect(screen.queryByText("OpenAI Hosted Tools")).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("Third party source API 格式"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "配置 Third party source" }),
    ).toBeVisible();
  });

  it("shows provider-owned readiness details in the model source card", () => {
    renderWizard([
      {
        id: "ready-source",
        name: "Ready source",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          apiFormat: "openai_responses",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [
              {
                model: "ready-model",
                contextWindow: 128000,
                supportsImage: true,
              },
            ],
          },
        },
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    expect(screen.getByText(/认证：API Key 已配置/)).toBeVisible();
    expect(screen.getByText(/模型目录：1 个/)).toBeVisible();
    expect(screen.getByText(/协议：openai_responses/)).toBeVisible();
    expect(screen.getByText(/能力：1\/1 个模型有能力摘要/)).toBeVisible();
    expect(screen.getByText(/工具\/投影：/)).toBeVisible();
  });

  it("reviews schema v2 route policy without inherited endpoint, protocol, or capabilities", () => {
    renderWizard([
      {
        id: "qwen-provider",
        name: "Qwen Provider",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://secret-upstream.invalid/v1",
          apiFormat: "openai_chat",
          auth: { OPENAI_API_KEY: "must-not-render" },
          modelCatalog: {
            models: [
              {
                model: "qwen3.8",
                apiFormat: "openai_responses",
                codexCache: { cacheMode: "qwen_context_cache" },
              },
            ],
          },
        },
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));

    const providerBadge = screen.getByTitle("Provider ID: qwen-provider");
    expect(providerBadge).toHaveTextContent("Qwen Provider");
    expect(screen.getByText(/Route 不保存这些字段/)).toBeVisible();
    expect(screen.queryByText("openai_chat")).not.toBeInTheDocument();
    expect(screen.queryByText("openai_responses")).not.toBeInTheDocument();
    expect(
      screen.queryByText("https://secret-upstream.invalid/v1"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("must-not-render")).not.toBeInTheDocument();
  });

  it("updates the open wizard from the latest Provider model catalog", () => {
    const source: Provider = {
      id: "codex-deepseek",
      name: "DeepSeek Responses",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "deepseek-v4-flash", contextWindow: 128000 },
            { model: "deepseek-v4-pro", contextWindow: 128000 },
          ],
        },
      },
    };
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wizard = (providers: Provider[]) => (
      <QueryClientProvider client={queryClient}>
        <CodexMultiRouterWizard
          open
          providers={providers}
          mode="create"
          onOpenChange={vi.fn()}
          onCreateProvider={vi.fn()}
          onOpenProviderConfig={vi.fn()}
          onOpenWorkspace={vi.fn()}
          onEnablePlan={vi.fn()}
        />
      </QueryClientProvider>
    );
    const view = render(wizard([source]));

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));
    expect(
      screen.queryByRole("checkbox", {
        name: "保留 deepseek-v4-flash-vision-exp",
      }),
    ).not.toBeInTheDocument();

    view.rerender(
      wizard([
        {
          ...source,
          settingsConfig: {
            ...source.settingsConfig,
            modelCatalog: {
              models: [
                { model: "deepseek-v4-flash", contextWindow: 1000000 },
                {
                  model: "deepseek-v4-flash-vision-exp",
                  contextWindow: 1000000,
                  inputModalities: ["text", "image"],
                },
                { model: "deepseek-v4-pro", contextWindow: 1000000 },
              ],
            },
          },
        },
      ]),
    );

    expect(
      screen.getByRole("checkbox", {
        name: "保留 deepseek-v4-flash-vision-exp",
      }),
    ).toBeChecked();
    expect(screen.getAllByText("1000000 ctx")).toHaveLength(3);
  });
});
