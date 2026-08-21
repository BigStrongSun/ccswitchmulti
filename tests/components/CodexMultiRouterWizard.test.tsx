import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import type { Provider } from "@/types";
import { CodexMultiRouterWizard } from "@/components/codex/CodexMultiRouterWizard";
import { CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY } from "@/lib/codexMultiRouterWizard";
import { providersApi } from "@/lib/api/providers";
import { codexSubagentV2Api } from "@/lib/api/codexSubagentV2";
import {
  fetchCodexOauthCachedModels,
  fetchCodexOauthModels,
  fetchModelsForConfig,
  probeCodexChatForConfig,
  probeCodexResponsesForConfig,
} from "@/lib/api/model-fetch";

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    add: vi.fn(),
    update: vi.fn(),
  },
}));

vi.mock("@/lib/api/codexSubagentV2", () => ({
  codexSubagentV2Api: {
    initializeProviderConfig: vi.fn(),
  },
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchCodexOauthCachedModels: vi.fn(),
  fetchCodexOauthModels: vi.fn(),
  fetchModelsForConfig: vi.fn(),
  probeCodexChatForConfig: vi.fn(),
  probeCodexResponsesForConfig: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: vi.fn(() => ({
    accounts: [],
    hasAnyAccount: false,
    isLoadingStatus: false,
  })),
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => (
    <div data-testid="wizard-codex-oauth-section">使用 ChatGPT 登录</div>
  ),
}));

// 构造最小 Codex provider，避免 UI 测试依赖真实数据库返回结构。
function provider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: overrides.id ?? "deepseek",
    name: overrides.name ?? "DeepSeek",
    category: overrides.category,
    settingsConfig: overrides.settingsConfig ?? {
      base_url: "https://api.deepseek.com/v1",
      auth: { OPENAI_API_KEY: "sk-test" },
      modelCatalog: { models: [{ model: "deepseek-chat" }] },
    },
    meta: overrides.meta,
  };
}

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  vi.mocked(fetchCodexOauthCachedModels).mockResolvedValue([]);
  vi.mocked(codexSubagentV2Api.initializeProviderConfig).mockImplementation(
    async (providerId) => {
      const persisted = vi
        .mocked(providersApi.add)
        .mock.calls.find(([candidate]) => candidate.id === providerId)?.[0];
      if (!persisted) {
        throw new Error("backend initializer requires the persisted provider");
      }
      return {
        ...persisted,
        settingsConfig: {
          ...persisted.settingsConfig,
          codexRouting: {
            ...persisted.settingsConfig.codexRouting,
            subagentV2: {
              schemaVersion: 1,
              selectionPolicy: "balanced",
              profiles: {
                "qwen3.6": {
                  model: "qwen3.6",
                  enabled: false,
                  questionnaire: {
                    taskStrengths: ["repository_exploration"],
                    optimization: "balanced",
                    writeScope: "read_only",
                    preference: "eligible",
                    reasoningEffort: "auto",
                  },
                },
              },
            },
          },
        },
      };
    },
  );
});

describe("CodexMultiRouterWizard", () => {
  it("keeps the first step focused on source selection and provider-owned configuration", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(screen.getByText("这里只选择模型源")).toBeInTheDocument();
    expect(screen.getByText(/都在各自 Provider 页面维护/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "配置 DeepSeek" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("子 Agent 候选")).not.toBeInTheDocument();
    expect(screen.queryByText(/这套向导会帮你完成/)).not.toBeInTheDocument();
    expect(screen.queryByText(/技术备注/)).not.toBeInTheDocument();
  });

  it("keeps runtime validation in the workspace and history repair as an independent Sessions action", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "启用并验证" })).toBeVisible();
    expect(
      screen.queryByText("启用后等待真实请求成功，再带你修复历史记录"),
    ).not.toBeInTheDocument();
  });

  it("shows provider names in route previews while keeping IDs in tooltips", () => {
    const source = provider({
      id: "5626e6b9-33cb-4c3b-8d16-af8176e16209",
      name: "DeepSeek Relay",
    });
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[source]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));

    const providerBadge = screen.getByTitle(
      "Provider ID: 5626e6b9-33cb-4c3b-8d16-af8176e16209",
    );
    expect(providerBadge).toHaveTextContent("DeepSeek Relay");
  });

  it("keeps V1 and V2 settings out of the four routing stages", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("button", { name: /Sub-Agent V1/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Sub-Agent V2/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "选择模型源" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "自动准备与验证" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "选择模型并预览路由" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "启用并验证" })).toBeVisible();
  });

  it("initializes a new default V2 plan before handing it to enable", async () => {
    const qwenSource = provider({
      id: "qwen-local",
      name: "Qwen Local",
      settingsConfig: {
        base_url: "https://qwen.example/v1",
        auth: { OPENAI_API_KEY: "sk-qwen" },
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    });
    const onEnablePlan = vi.fn();
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[qwenSource]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={onEnablePlan}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "保存并发布" }));

    await waitFor(() => expect(providersApi.add).toHaveBeenCalledTimes(1));
    const persisted = vi.mocked(providersApi.add).mock.calls[0][0];
    expect(persisted.settingsConfig.codexRouting.subagentVersion).toBe("v2");

    await waitFor(() =>
      expect(codexSubagentV2Api.initializeProviderConfig).toHaveBeenCalledWith(
        persisted.id,
      ),
    );
    const initialized = await vi.mocked(
      codexSubagentV2Api.initializeProviderConfig,
    ).mock.results[0].value;
    expect(initialized).toMatchObject({
      id: persisted.id,
      name: persisted.name,
      settingsConfig: expect.objectContaining({
        base_url: persisted.settingsConfig.base_url,
        auth: persisted.settingsConfig.auth,
      }),
    });
    expect(
      initialized.settingsConfig.codexRouting.subagentV2.profiles,
    ).toHaveProperty("qwen3.6");

    fireEvent.click(
      await screen.findByRole("button", { name: "启用这个多路路由" }),
    );
    await waitFor(() => expect(onEnablePlan).toHaveBeenCalledWith(initialized));
    expect(onEnablePlan.mock.calls[0][0]).toBe(initialized);
  });

  it("keeps the wizard controls inside small app windows", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    const shell = screen.getByTestId("codex-multirouter-wizard-shell");
    const body = screen.getByTestId("codex-multirouter-wizard-body");
    const footer = screen.getByRole("button", { name: "下一步" }).parentElement
      ?.parentElement;

    expect(shell).toHaveClass("flex", "max-h-full", "flex-col");
    expect(body).toHaveClass("min-h-0", "flex-1", "overflow-hidden");
    expect(footer).toHaveClass("shrink-0");
  });

  it("opens, navigates steps, and stores dismissed flag when skipped", () => {
    const onOpenChange = vi.fn();

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={onOpenChange}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(screen.getAllByText("选择模型源").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getAllByText("自动准备与验证").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    expect(localStorage.getItem(CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY)).toBe(
      "true",
    );
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("renders an existing plan when its historical model catalog contains invalid entries", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "existing-plan",
            name: "Existing Plan",
            settingsConfig: {
              codexRouting: { schemaVersion: 2, enabled: true, routes: [] },
              modelCatalog: {
                models: [null, "stale", { model: "deepseek-chat" }],
              },
            } as Provider["settingsConfig"],
          }),
          provider(),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "选择模型源" }),
    ).toBeInTheDocument();
  });

  it("guides official Codex sources to configure ChatGPT OAuth in provider config step", () => {
    const onOpenProviderConfig = vi.fn();
    const official = provider({
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {},
    });
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[official]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={onOpenProviderConfig}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "配置 OpenAI Official" }),
    );
    expect(onOpenProviderConfig).toHaveBeenCalledWith(official);
    expect(
      screen.queryByLabelText("OpenAI Official API 格式"),
    ).not.toBeInTheDocument();
  });

  it("does not reset to the intro step when parent rerenders with a new providers array", () => {
    const deepseekProvider = provider();
    const { rerender } = renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[deepseekProvider]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(
      screen.getByRole("heading", { name: "自动准备与验证" }),
    ).toBeInTheDocument();

    rerender(
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: { queries: { retry: false } },
          })
        }
      >
        <CodexMultiRouterWizard
          open
          providers={[{ ...deepseekProvider }]}
          onOpenChange={vi.fn()}
          onCreateProvider={vi.fn()}
          onOpenWorkspace={vi.fn()}
          onEnablePlan={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("heading", { name: "自动准备与验证" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("这套向导会帮你完成 7 件事"),
    ).not.toBeInTheDocument();
  });

  it("keeps provider protocol editing out of the wizard when a stale provider refetch arrives", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const staleQwen = provider({
      id: "qwen-local",
      name: "Qwen Local",
      meta: { apiFormat: "openai_responses" },
      settingsConfig: {
        base_url: "https://qwen.example/v1",
        auth: { OPENAI_API_KEY: "sk-qwen" },
        apiFormat: "openai_responses",
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    });
    const renderWizard = (source: Provider) => (
      <QueryClientProvider client={queryClient}>
        <CodexMultiRouterWizard
          open
          providers={[source]}
          onOpenChange={vi.fn()}
          onCreateProvider={vi.fn()}
          onOpenProviderConfig={vi.fn()}
          onOpenWorkspace={vi.fn()}
          onEnablePlan={vi.fn()}
        />
      </QueryClientProvider>
    );
    const { rerender } = render(renderWizard(staleQwen));

    expect(
      screen.queryByLabelText("Qwen Local API 格式"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "配置 Qwen Local" }),
    ).toBeInTheDocument();

    // 模拟后台 provider query 在用户选择之后返回数据库里的旧 Responses 快照。
    rerender(renderWizard({ ...staleQwen }));

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "配置 Qwen Local" }),
      ).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));
    expect(screen.queryByText("openai_responses")).not.toBeInTheDocument();
    expect(
      screen.getByText(/协议、连接地址、凭据和模型能力始终读取目标/),
    ).toBeInTheDocument();
  });

  it("marks catalog-only providers as continuable instead of requiring full config", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "catalog-only",
            name: "Catalog Only",
            settingsConfig: {
              modelCatalog: { models: [{ model: "manual-model" }] },
            },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(screen.getByText("1 个模型")).toBeInTheDocument();
    expect(screen.queryByText(/未配置在线获取参数/)).not.toBeInTheDocument();
    expect(screen.queryByText("需补全配置")).not.toBeInTheDocument();
  });

  it("refreshes providers that already have modelCatalog and marks unchanged lists", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "deepseek-chat", ownedBy: null },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    expect(await screen.findByText("无模型列表更新")).toBeInTheDocument();
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
    expect(providersApi.update).toHaveBeenCalledTimes(1);
  });

  it("refreshes an official OAuth catalog with its bound account and appends new models", async () => {
    vi.mocked(fetchCodexOauthModels).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: "openai", contextWindow: 272000 },
      { id: "gpt-5.6-sol", ownedBy: "openai", contextWindow: 372000 },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);
    const officialProvider = provider({
      id: "codex-official",
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
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[officialProvider]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    await waitFor(() =>
      expect(fetchCodexOauthModels).toHaveBeenCalledWith("account-56"),
    );
    await waitFor(() => expect(providersApi.update).toHaveBeenCalledTimes(1));
    const savedProvider = vi.mocked(providersApi.update).mock.calls[0][0];
    expect(
      savedProvider.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["gpt-5.5", "gpt-5.6-sol"]);
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
  });

  it("keeps the last OAuth catalog when the dynamic model request fails", async () => {
    vi.mocked(fetchCodexOauthModels).mockRejectedValueOnce(
      new Error("temporary OAuth endpoint failure"),
    );
    const officialProvider = provider({
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      meta: { providerType: "codex_oauth" },
      settingsConfig: {
        modelCatalog: { models: [{ model: "gpt-5.5" }] },
      },
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[officialProvider]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    expect(
      await screen.findByText(/OAuth 模型列表获取失败，已保留现有目录/),
    ).toBeInTheDocument();
    expect(providersApi.update).not.toHaveBeenCalled();
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
  });

  it("uses local Codex cache when OAuth model request fails for an empty official source", async () => {
    vi.mocked(fetchCodexOauthModels).mockRejectedValueOnce(
      new Error("error sending request for url"),
    );
    vi.mocked(fetchCodexOauthCachedModels).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: "Codex", contextWindow: 256000 },
      { id: "gpt-5.6-luna", ownedBy: "Codex", contextWindow: 256000 },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);
    const officialProvider = provider({
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      meta: { providerType: "codex_oauth" },
      settingsConfig: { modelCatalog: { models: [] } },
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[officialProvider]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    expect(
      await screen.findByText(/已使用本地 Codex 模型缓存写入 2 个模型/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/OAuth 在线模型列表获取失败，已使用本地缓存/),
    ).toBeInTheDocument();
    const savedProvider = vi.mocked(providersApi.update).mock.calls[0][0];
    expect(
      savedProvider.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["gpt-5.5", "gpt-5.6-luna"]);
  });

  it("excludes an unchecked provider from the generated MultiRouter plan", async () => {
    const secondSource = provider({
      id: "qwen-local",
      name: "Qwen Local",
      settingsConfig: {
        base_url: "https://qwen.example/v1",
        auth: { OPENAI_API_KEY: "sk-qwen" },
        modelCatalog: { models: [{ model: "qwen3.6" }] },
      },
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider(), secondSource]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByLabelText("使用 Qwen Local 作为模型源"));
    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));
    fireEvent.click(
      screen.getAllByRole("button", { name: "保存并发布" }).at(-1)!,
    );

    await waitFor(() => expect(providersApi.add).toHaveBeenCalledTimes(1));
    const savedProvider = vi.mocked(providersApi.add).mock.calls[0][0];
    expect(
      savedProvider.settingsConfig.codexRouting.routes.map(
        (route: { id: string }) => route.id,
      ),
    ).not.toContain("qwen-local");
    expect(savedProvider.settingsConfig).not.toHaveProperty("modelCatalog");
  });

  it("keeps provider curated models when wizard refresh sees extra upstream models", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "deepseek-chat", ownedBy: null, contextWindow: 128000 },
      { id: "deepseek-reasoner", ownedBy: null, contextWindow: 64000 },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            settingsConfig: {
              base_url: "https://api.deepseek.com/v1",
              auth: { OPENAI_API_KEY: "sk-test" },
              modelCatalog: {
                models: [{ model: "deepseek-chat" }],
                spawnAgentModels: ["deepseek-chat", "deepseek-reasoner"],
              },
            },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    await waitFor(() => expect(providersApi.update).toHaveBeenCalledTimes(1));
    const savedProvider = vi.mocked(providersApi.update).mock.calls[0][0];
    expect(
      savedProvider.settingsConfig.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["deepseek-chat"]);
    expect(savedProvider.settingsConfig.modelCatalog.spawnAgentModels).toEqual([
      "deepseek-chat",
    ]);
  });

  it("falls back to data-plane models for AgentPlan without AK/SK when API Key exists", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "ark-code-latest", ownedBy: "volcengine" },
      { id: "doubao-seed-1.6", ownedBy: "volcengine" },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "ark-agentplan",
            name: "火山Agentplan",
            settingsConfig: {
              base_url: "https://ark.cn-beijing.volces.com/api/coding/v3",
              auth: { OPENAI_API_KEY: "sk-volc" },
              modelCatalog: {
                models: [{ model: "ark-code-latest" }],
              },
            },
            meta: { partnerPromotionKey: "volcengine_agentplan" },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    await waitFor(() => {
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://ark.cn-beijing.volces.com/api/coding/v3",
        "sk-volc",
        false,
        undefined,
        undefined,
        undefined,
      );
      expect(providersApi.update).toHaveBeenCalledTimes(1);
    });
    expect(
      await screen.findByText("读取成功，无模型列表更新，仍为 1 个模型。"),
    ).toBeInTheDocument();
  });

  it("skips AgentPlan model fetch when both inference key and AK/SK are missing", async () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "ark-agentplan",
            name: "火山Agentplan",
            settingsConfig: {
              base_url: "https://ark.cn-beijing.volces.com/api/coding/v3",
              auth: { OPENAI_API_KEY: "" },
              modelCatalog: {
                models: [{ model: "ark-code-latest" }],
              },
            },
            meta: { partnerPromotionKey: "volcengine_agentplan" },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    await waitFor(() => {
      expect(fetchModelsForConfig).not.toHaveBeenCalled();
      expect(providersApi.update).not.toHaveBeenCalled();
    });
  });

  it("refreshes AgentPlan models through Volcengine OpenAPI when AK/SK exists", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "doubao-seed-1.6", ownedBy: "volcengine" },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "ark-agentplan",
            name: "火山Agentplan",
            settingsConfig: {
              base_url: "https://ark.cn-beijing.volces.com/api/coding/v3",
              auth: { OPENAI_API_KEY: "sk-volc" },
              modelCatalog: {
                models: [{ model: "ark-code-latest" }],
              },
            },
            meta: {
              partnerPromotionKey: "volcengine_agentplan",
              usage_script: {
                enabled: true,
                language: "javascript",
                code: "",
                accessKeyId: "AKLTtest",
                secretAccessKey: "secret",
              },
            },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    await waitFor(() => {
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://ark.cn-beijing.volces.com/api/coding/v3",
        "sk-volc",
        false,
        undefined,
        undefined,
        {
          action: "ListArkAgentPlanModel",
          accessKeyId: "AKLTtest",
          secretAccessKey: "secret",
        },
      );
      expect(providersApi.update).toHaveBeenCalledTimes(1);
    });
    expect(
      await screen.findByText("读取成功，无模型列表更新，仍为 1 个模型。"),
    ).toBeInTheDocument();
  });

  it("keeps previous model selections without re-adding newly fetched provider models", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "model-a", ownedBy: null },
      { id: "model-b", ownedBy: null },
      { id: "model-c", ownedBy: null },
    ]);
    vi.mocked(providersApi.update).mockResolvedValueOnce(true);

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "relay",
            name: "Relay",
            settingsConfig: {
              base_url: "https://relay.example/v1",
              auth: { OPENAI_API_KEY: "sk-test" },
              modelCatalog: {
                models: [
                  { model: "model-a", upstreamModel: "model-a" },
                  { model: "model-b", upstreamModel: "model-b" },
                ],
              },
            },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));
    fireEvent.click(screen.getByLabelText("保留 model-b"));
    expect(screen.getByLabelText("保留 model-b")).not.toBeChecked();

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    expect(await screen.findByText("无模型列表更新")).toBeInTheDocument();
    expect(screen.queryByText(/新增 1: model-c/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));
    expect(screen.getByLabelText("保留 model-a")).toBeChecked();
    expect(screen.getByLabelText("保留 model-b")).not.toBeChecked();
    expect(screen.queryByLabelText("保留 model-c")).not.toBeInTheDocument();
  });

  it("opens a provider config page from the model fetch cards", () => {
    const onOpenProviderConfig = vi.fn();
    const source = provider();

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[source]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={onOpenProviderConfig}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "打开 DeepSeek 配置页" }),
    );

    expect(onOpenProviderConfig).toHaveBeenCalledWith(source);
  });

  it("does not expose inferred protocol details for official sources in the source picker", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "openai-official-backup",
            name: "OpenAI Official Backup",
            category: "official",
            meta: { apiFormat: "openai_chat" },
            settingsConfig: {
              modelCatalog: {
                models: [{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }],
              },
            },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(
      screen.getAllByText(/OpenAI Official Backup/).length,
    ).toBeGreaterThan(0);
    expect(
      screen.queryByText(/API 格式：Responses API（向导推断/),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "配置 OpenAI Official Backup" }),
    ).toBeInTheDocument();
  });

  it("keeps manually locked chat protocol on the Provider instead of a Route snapshot", async () => {
    vi.mocked(probeCodexResponsesForConfig).mockResolvedValueOnce({
      ok: true,
      status: 200,
      url: "https://relay.example/v1/responses",
      model: "gpt-5.5",
      detail: "ok",
    });
    vi.mocked(probeCodexChatForConfig).mockResolvedValueOnce({
      ok: true,
      status: 200,
      url: "https://relay.example/v1/chat/completions",
      model: "gpt-5.5",
      detail: "ok",
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[
          provider({
            id: "relay",
            name: "Relay",
            settingsConfig: {
              base_url: "https://relay.example/v1",
              auth: { OPENAI_API_KEY: "sk-test" },
              apiFormat: "openai_chat",
              modelCatalog: {
                models: [{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }],
              },
            },
            meta: { apiFormat: "openai_chat", apiFormatSource: "manual" },
          }),
        ]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses 连通性" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    expect(
      await screen.findByText("状态机：connectivityPassed"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));
    fireEvent.click(
      screen.getAllByRole("button", { name: "保存并发布" }).at(-1)!,
    );

    await waitFor(() => {
      expect(providersApi.add).toHaveBeenCalledTimes(1);
    });
    const savedProvider = vi.mocked(providersApi.add).mock.calls[0][0];
    expect(savedProvider.settingsConfig.codexRouting.routes[0]).toMatchObject({
      targetProviderId: "relay",
    });
    expect(
      savedProvider.settingsConfig.codexRouting.routes[0],
    ).not.toHaveProperty("upstream");
  });

  it("stays in needSources state when advancing without model sources", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));

    expect(screen.getByText("状态机：needSources")).toBeInTheDocument();
    expect(screen.getByText(/请先添加一个普通 Codex/)).toBeInTheDocument();
  });

  it("moves to saveFailed state when publishing the generated plan fails", async () => {
    vi.mocked(providersApi.add).mockRejectedValueOnce(new Error("db locked"));

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));
    const publishButtons = screen.getAllByRole("button", {
      name: "保存并发布",
    });
    fireEvent.click(publishButtons[publishButtons.length - 1]);

    expect(await screen.findByText("状态机：saveFailed")).toBeInTheDocument();
    expect(screen.getByText("MultiRouter 保存失败")).toBeInTheDocument();
    expect(screen.getAllByText("db locked").length).toBeGreaterThan(0);
  });

  it("saves a renamed curated plan without requiring manual subagent selection", async () => {
    const onOpenChange = vi.fn();
    const source = provider({
      id: "relay",
      name: "Relay",
      settingsConfig: {
        base_url: "https://relay.example/v1",
        auth: { OPENAI_API_KEY: "sk-test" },
        modelCatalog: {
          models: [
            { model: "model-a", upstreamModel: "model-a" },
            { model: "model-b", upstreamModel: "model-b" },
            { model: "model-c", upstreamModel: "model-c" },
          ],
        },
      },
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[source]}
        onOpenChange={onOpenChange}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));
    fireEvent.change(screen.getByLabelText("MultiRouter 名称"), {
      target: { value: "Work MultiRouter" },
    });

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));
    fireEvent.click(screen.getByLabelText("保留 model-b"));
    fireEvent.click(screen.getAllByTitle("上移")[1]);

    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));
    fireEvent.click(
      screen.getAllByRole("button", { name: "保存并发布" }).at(-1)!,
    );

    await waitFor(() => {
      expect(providersApi.add).toHaveBeenCalledTimes(1);
    });
    const savedProvider = vi.mocked(providersApi.add).mock.calls[0][0];
    expect(savedProvider.name).toBe("Work MultiRouter");
    expect(savedProvider.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(savedProvider.settingsConfig.codexRouting.spawnAgentModels).toEqual(
      [],
    );
    expect(
      savedProvider.settingsConfig.codexRouting.routes[0].modelSelection,
    ).toEqual({ mode: "include", models: ["model-c", "model-a"] });
  });

  it("confirms and probes both Chat and Responses connectivity before recording pass state", async () => {
    vi.mocked(probeCodexResponsesForConfig).mockResolvedValueOnce({
      ok: true,
      status: 200,
      url: "https://api.deepseek.com/v1/responses",
      model: "deepseek-chat",
      detail: "ok",
    });
    vi.mocked(probeCodexChatForConfig).mockResolvedValueOnce({
      ok: true,
      status: 200,
      url: "https://api.deepseek.com/v1/chat/completions",
      model: "deepseek-chat",
      detail: "ok",
    });

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses 连通性" }),
    );
    expect(screen.getByText("确认开始连通性测试")).toBeInTheDocument();
    expect(screen.getAllByRole("dialog").at(-1)).toHaveClass("z-[200]");
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(
      await screen.findByText("状态机：connectivityPassed"),
    ).toBeInTheDocument();
    expect(probeCodexResponsesForConfig).toHaveBeenCalledWith(
      "https://api.deepseek.com/v1",
      "sk-test",
      "deepseek-chat",
      false,
      undefined,
    );
    expect(probeCodexChatForConfig).toHaveBeenCalledWith(
      "https://api.deepseek.com/v1",
      "sk-test",
      "deepseek-chat",
      false,
      undefined,
    );
  });

  it("shows fetched model exceptions in the wizard issue panel", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
      new Error("upstream /models timeout"),
    );

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "自动获取并写入模型列表" }),
    );

    expect(await screen.findByText("模型列表获取失败")).toBeInTheDocument();
    expect(
      screen.getAllByText(/upstream \/models timeout/).length,
    ).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("可继续")).toBeInTheDocument();
    consoleError.mockRestore();
  });

  it("shows responses probe command exceptions and blocks continuation", async () => {
    vi.mocked(probeCodexResponsesForConfig).mockRejectedValueOnce(
      new Error("ipc invoke failed"),
    );

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider({ meta: { apiFormat: "openai_responses" } })]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "自动准备与验证" }));
    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses 连通性" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByText("连通性探测命令异常")).toBeInTheDocument();
    expect(screen.getByText("ipc invoke failed")).toBeInTheDocument();
    expect(screen.getByText("需处理后继续")).toBeInTheDocument();
    expect(screen.getByText("状态机：connectivityFailed")).toBeInTheDocument();
  });

  it("closes the overlay after enabling so the status-page handoff can continue", async () => {
    const onOpenChange = vi.fn();
    const onEnablePlan = vi.fn().mockResolvedValue(undefined);

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={onOpenChange}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={onEnablePlan}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "启用并验证" }));
    const publishButtons = screen.getAllByRole("button", {
      name: "保存并发布",
    });
    fireEvent.click(publishButtons[publishButtons.length - 1]);

    expect(
      await screen.findByText(/启用成功后向导会自动关闭/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "启用这个多路路由" }));

    expect(onEnablePlan).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("状态机：completed")).toBeInTheDocument();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
