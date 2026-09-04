import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import * as modelFetchApi from "@/lib/api/model-fetch";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { providersApi } from "@/lib/api/providers";
import {
  buildWizardConnectivityResultsFromBatchOutcome,
  CodexMultiRouterWizard,
} from "./CodexMultiRouterWizard";

const {
  preflightCodexProviderProtocolCompatibility,
  prepareCodexProviderSetBatch,
  commitCodexProviderSetBatch,
} = vi.hoisted(() => ({
  preflightCodexProviderProtocolCompatibility: vi.fn(),
  prepareCodexProviderSetBatch: vi.fn(),
  commitCodexProviderSetBatch: vi.fn(),
}));

vi.mock("@/lib/api/protocol-compatibility", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("@/lib/api/protocol-compatibility")
  >()),
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
  options?: {
    mode?: "create" | "edit";
    planId?: string;
    onEnablePlan?: (provider: Provider) => Promise<void>;
  },
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = (
    open = true,
    nextProviders = providers,
    nextOptions = options,
  ) => (
    <QueryClientProvider client={queryClient}>
      <CodexMultiRouterWizard
        open={open}
        providers={nextProviders}
        mode={nextOptions?.mode ?? "create"}
        planId={nextOptions?.planId}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={nextOptions?.onEnablePlan ?? vi.fn()}
      />
    </QueryClientProvider>
  );
  const rendered = render(view());
  return {
    ...rendered,
    rerenderWizard: (
      open = true,
      nextProviders = providers,
      nextOptions = options,
    ) => rendered.rerender(view(open, nextProviders, nextOptions)),
  };
}

describe("CodexMultiRouterWizard", () => {
  it("does not mark a new draft enabled when the previous target finishes enabling", async () => {
    const plan: Provider = {
      id: "pending-enable",
      name: "Pending Enable",
      settingsConfig: {
        codexRouting: { schemaVersion: 2, enabled: true, routes: [] },
      },
    };
    let completeEnable!: () => void;
    const onEnablePlan = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          completeEnable = resolve;
        }),
    );
    const { rerenderWizard } = renderWizard([plan], {
      mode: "edit",
      planId: plan.id,
      onEnablePlan,
    });
    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));
    fireEvent.click(screen.getByRole("button", { name: "启用这个多路路由" }));
    expect(onEnablePlan).toHaveBeenCalledWith(plan);
    rerenderWizard(false);
    rerenderWizard(true, [plan], { mode: "create" });
    const statusBefore = screen.getByText(/状态机：/).textContent;
    await act(async () => {
      completeEnable();
    });
    expect(screen.queryByText("状态机：enabled")).not.toBeInTheDocument();
    expect(screen.getByText(/状态机：/).textContent).toBe(statusBefore);
  });

  it("isolates an edited plan from a new draft and ignores its late model fetch", async () => {
    const source: Provider = {
      id: "isolated-source",
      name: "Isolated Source",
      meta: { codexProtocolMode: "manual", apiFormat: "openai_responses" },
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: "original" }] },
      },
    };
    const plan: Provider = {
      id: "existing-router",
      name: "Existing Router",
      settingsConfig: {
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          routes: [
            {
              id: "route",
              targetProviderId: source.id,
              modelSelection: { mode: "all" },
              authPolicy: { source: "provider_config" },
            },
          ],
        },
      },
    };
    let finishFetch!: (models: modelFetchApi.FetchedModel[]) => void;
    const fetchSpy = vi
      .spyOn(modelFetchApi, "fetchModelsForConfig")
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishFetch = resolve;
          }),
      );
    prepareCodexProviderSetBatch.mockResolvedValue({
      digest: "isolated",
      sourcePreviews: [],
      blocked: false,
    });
    commitCodexProviderSetBatch.mockImplementation(
      async (_sources, router) => ({
        router,
        sourceSnapshots: [],
        projections: [],
        status: "committed",
      }),
    );
    const providers = [
      source,
      { ...source, id: "second-source", name: "Second Source" },
      plan,
    ];
    const { rerenderWizard } = renderWizard(providers, {
      mode: "edit",
      planId: plan.id,
    });
    fireEvent.click(screen.getByRole("button", { name: "同步模型目录" }));
    fireEvent.click(screen.getByRole("button", { name: "自动获取模型列表" }));
    expect(fetchSpy).toHaveBeenCalled();
    rerenderWizard(false);
    rerenderWizard(true, providers, { mode: "create" });
    await act(async () => {
      finishFetch([{ id: "late-model", ownedBy: null }]);
    });
    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    fireEvent.click(screen.getByRole("button", { name: "开始兼容性深度探测" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    await screen.findByText("状态机：connectivityPartial");
    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));
    fireEvent.click(screen.getByRole("button", { name: "保存并发布" }));
    await waitFor(() => expect(commitCodexProviderSetBatch).toHaveBeenCalled());
    const [sources, router] = commitCodexProviderSetBatch.mock.calls.at(-1)!;
    expect(router.id).not.toBe(plan.id);
    expect(sources).toHaveLength(2);
    expect(
      sources[0].provider.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["original"]);
    fetchSpy.mockRestore();
  });

  it("retains the draft page when closed and reopened in the same application session", () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const source: Provider = {
      id: "cache-source",
      name: "Cache Source",
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: "model-a" }] },
      },
    };
    const props = {
      providers: [source],
      mode: "create" as const,
      onOpenChange: vi.fn(),
      onCreateProvider: vi.fn(),
      onOpenProviderConfig: vi.fn(),
      onOpenWorkspace: vi.fn(),
      onEnablePlan: vi.fn(),
    };
    const view = (open: boolean) => (
      <QueryClientProvider client={queryClient}>
        <CodexMultiRouterWizard {...props} open={open} />
      </QueryClientProvider>
    );
    const { rerender } = render(view(true));
    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    rerender(view(false));
    rerender(view(true));
    expect(screen.getByRole("heading", { name: "协议深探测" })).toBeVisible();
  });

  it("can exclude a failed model and save verified receipts without probing again", async () => {
    const source: Provider = {
      id: "mixed-probe",
      name: "Mixed Probe",
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: "good" }, { model: "bad" }] },
      },
    };
    preflightCodexProviderProtocolCompatibility.mockResolvedValueOnce({
      provider: source,
      receiptIds: ["receipt-good", "receipt-bad"],
      observations: [],
      protocolApplied: false,
      adaptationPreview: {
        persistence: "blocked",
        status: "blocked",
        models: [],
      },
      records: ["good", "bad"].map((model) => ({
        target: {
          public_model: model,
          upstream_model: model,
          transport: "open_ai_responses",
        },
        result: {
          readiness: model === "good" ? "verified" : "unverified",
          selected_transport: model === "good" ? "open_ai_responses" : null,
          branches: [],
        },
      })),
    });
    prepareCodexProviderSetBatch.mockResolvedValue({
      digest: "selected",
      sourcePreviews: [],
      blocked: false,
    });
    commitCodexProviderSetBatch.mockImplementation(
      async (_sources, router) => ({
        router,
        sourceSnapshots: [],
        projections: [],
        status: "committed",
      }),
    );
    const { rerenderWizard } = renderWizard([source]);
    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    fireEvent.click(screen.getByRole("button", { name: "开始兼容性深度探测" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    await screen.findByText("状态机：connectivityFailed");
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    rerenderWizard(false);
    rerenderWizard(true, [{ ...source, name: "Renamed Probe" }]);
    expect(screen.getByRole("heading", { name: "协议深探测" })).toBeVisible();
    expect(
      screen.getByRole("checkbox", { name: "保留 Mixed Probe / bad" }),
    ).toBeChecked();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "保留 Mixed Probe / bad" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "选择模型" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));
    fireEvent.click(screen.getByRole("button", { name: "保存并发布" }));
    await waitFor(() => expect(commitCodexProviderSetBatch).toHaveBeenCalled());
    const sources = commitCodexProviderSetBatch.mock.calls.at(-1)![0];
    expect(sources[0].provider.name).toBe("Renamed Probe");
    expect(sources[0].receiptIds).toEqual(["receipt-good"]);
    expect(sources[0].provider.settingsConfig.modelCatalog.models).toEqual([
      { model: "good" },
      { model: "bad", enabled: false },
    ]);
    expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalledTimes(
      1,
    );
    expect(
      source.settingsConfig.modelCatalog.models[1].enabled,
    ).toBeUndefined();
  });

  it("invalidates only the edited source and never allows an empty retained selection", async () => {
    const makeSource = (id: string): Provider => ({
      id,
      name: id,
      meta: { codexProtocolMode: "manual", apiFormat: "openai_responses" },
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: `${id}-model` }] },
      },
    });
    const a = makeSource("source-a");
    const b = makeSource("source-b");
    const { rerenderWizard } = renderWizard([a, b]);
    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    fireEvent.click(screen.getByRole("button", { name: "开始兼容性深度探测" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    await screen.findByText("状态机：connectivityPartial");
    rerenderWizard(false);
    const changed = {
      ...a,
      settingsConfig: {
        ...a.settingsConfig,
        baseUrl: "https://changed.invalid/v1",
      },
    };
    rerenderWizard(true, [changed, b]);
    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    expect(
      screen.getByText(/模型源配置已修改，旧探测结果已失效/),
    ).toBeVisible();
    expect(
      screen.getByRole("checkbox", { name: "保留 source-b / *" }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "取消所有失败模型" }));
    fireEvent.click(
      screen.getByRole("checkbox", { name: "保留 source-b / *" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "协议深探测" })).toBeVisible();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "保留 source-b / *" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "选择模型" })).toBeVisible();
  });

  it("turns a missing batch probe source into a named failure instead of an all-zero summary", () => {
    const source: Provider = {
      id: "qwen-local",
      name: "Qwen Local",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://example.invalid/v1",
        auth: { OPENAI_API_KEY: "test-only" },
        modelCatalog: { models: [{ model: "qwen3.8" }] },
      },
    };

    expect(
      buildWizardConnectivityResultsFromBatchOutcome(
        [source],
        { outcomes: [], sources: [] },
        false,
      ),
    ).toEqual([
      expect.objectContaining({
        providerId: "qwen-local",
        providerName: "Qwen Local",
        status: "fail",
        canContinue: false,
        detail: "后端没有返回该模型源的兼容性探测结果。",
      }),
    ]);
  });

  it("keeps legacy V1 controls out while exposing the dedicated Sub-Agent page", () => {
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
      screen.getByRole("button", { name: "Sub-Agent 与工具" }),
    ).toBeVisible();
  });

  it("presents the complete guided setup as single-responsibility pages", () => {
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

    for (const pageName of [
      "开始配置",
      "环境检查",
      "模型源就绪",
      "选择模型源",
      "同步模型目录",
      "协议深探测",
      "选择模型",
      "模型顺序",
      "推理设置",
      "Sub-Agent 与工具",
      "路由确认",
      "保存并启用",
      "真实请求验收",
    ]) {
      expect(screen.getByRole("button", { name: pageName })).toBeVisible();
    }
    expect(
      screen.getByText(/最终确认前不会启用或覆盖当前 Codex/),
    ).toBeVisible();
  });

  it("explains the production-equivalent deep-probe contract without legacy shallow-probe limits", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));

    const contract = screen.getByText(/每个普通 Provider\/模型会分别验证/);
    expect(contract).toHaveTextContent("流式 SSE");
    expect(contract).toHaveTextContent("推理语义");
    expect(contract).toHaveTextContent("强制工具调用");
    expect(contract).toHaveTextContent("工具结果续接");
    expect(screen.queryByText(/浅探测|1024/)).not.toBeInTheDocument();
  });

  it("previews a blocked future page without mutating workflow progress", () => {
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

    expect(screen.getByText("状态机：opened")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));

    expect(screen.getByRole("heading", { name: "保存并启用" })).toBeVisible();
    expect(screen.getByText("当前步骤暂不可编辑")).toBeVisible();
    expect(screen.getByText("状态机：opened")).toBeVisible();
    expect(screen.queryByText("状态机：enablePrompt")).not.toBeInTheDocument();
  });

  it("counts only automatic models in the deep-probe progress denominator", async () => {
    preflightCodexProviderProtocolCompatibility.mockImplementationOnce(
      () => new Promise(() => undefined),
    );
    const { unmount } = renderWizard([
      {
        id: "qwen-local",
        name: "Qwen Local",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: { models: [{ model: "qwen3.8" }] },
        },
      },
      {
        id: "codex-official",
        name: "OpenAI Official",
        category: "official",
        meta: { providerType: "codex_oauth" },
        settingsConfig: {
          modelCatalog: {
            models: [{ model: "gpt-5.6-sol" }, { model: "gpt-5.6-luna" }],
          },
        },
      },
      {
        id: "mixed-manual",
        name: "Mixed Manual",
        category: "custom",
        meta: {
          codexProtocolMode: "manual",
          codexProtocolOverrides: { "kimi-k3": "openai_chat" },
        },
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [{ model: "kimi-k3" }, { model: "glm-5.3" }],
          },
        },
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    fireEvent.click(screen.getByRole("button", { name: "开始兼容性深度探测" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByText(/正在验证模型 0\/2/)).toBeInTheDocument();
    expect(screen.queryByText(/正在验证模型 0\/5/)).not.toBeInTheDocument();
    unmount();
  });

  it("keeps the final save entry visible when no model source exists", () => {
    renderWizard([]);

    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));

    expect(screen.getByRole("button", { name: "保存并发布" })).toBeVisible();
    expect(screen.getByText(/尚未选择模型源，保存入口仍保留/)).toBeVisible();
    expect(screen.getByText("当前步骤暂不可编辑")).toBeVisible();
  });

  it("coalesces rapid saves and updates the same plan after the first save", async () => {
    let resolveCommit:
      | ((value: {
          preview: Record<string, unknown>;
          router: Provider;
          sourceSnapshots: never[];
          projections: never[];
          status: "committed";
          projectionErrorCode: null;
        }) => void)
      | undefined;
    const firstCommit = new Promise<{
      preview: Record<string, unknown>;
      router: Provider;
      sourceSnapshots: never[];
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
        sourceSnapshots: [],
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
    fireEvent.click(screen.getByRole("button", { name: "协议深探测" }));
    fireEvent.click(screen.getByRole("button", { name: "开始兼容性深度探测" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    await screen.findByText("状态机：connectivityPartial");
    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));
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
      sourceSnapshots: [],
      projections: [],
      status: "committed",
      projectionErrorCode: null,
    });
    await waitFor(() =>
      expect(screen.getByText("状态机：published")).toBeVisible(),
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

    fireEvent.click(screen.getByRole("button", { name: "选择模型源" }));
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

    fireEvent.click(screen.getByRole("button", { name: "选择模型源" }));
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

    fireEvent.click(screen.getByRole("button", { name: "模型源就绪" }));
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

    fireEvent.click(screen.getByRole("button", { name: "路由确认" }));

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

    fireEvent.click(screen.getByRole("button", { name: "选择模型" }));
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
