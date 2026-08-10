import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { CodexMultiRouterWizard } from "./CodexMultiRouterWizard";
import { CodexRouterWorkspacePage } from "./CodexRouterWorkspacePage";

const previewResponse = vi.hoisted(() => ({
  providerKind: "third_party" as const,
  requestedRoleName: "repo-reader",
  effectiveRoleName: "repo-reader",
  description: "Backend-generated repository exploration role.",
  developerInstructions: "Inspect the repository and report evidence.",
  nicknameCandidates: ["Scout", "Reader"],
  model: "deepseek-v4-flash",
  modelProvider: "codex_model_router_v2",
  modelReasoningEffort: "medium" as const,
  modelContextWindow: 128000,
  tomlPreview: '[agents.repo-reader]\nmodel = "deepseek-v4-flash"',
  warnings: [],
}));

// The V2 editor is an IPC consumer: this is the only mocked boundary.  The
// fixture deliberately mirrors the complete public backend response instead of
// reproducing compilation logic in the browser test.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(previewResponse),
}));

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: () => ({
    accounts: [],
    hasAnyAccount: false,
    isLoadingStatus: false,
  }),
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

vi.mock("@/lib/api", () => ({
  providersApi: { add: vi.fn(), update: vi.fn() },
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

vi.mock("@/lib/query/usage", () => ({
  usageKeys: { all: ["usage"] },
  useCodexSubagentUsageStats: () => ({
    data: {
      totals: { sessions: 0, inputTokens: 0, outputTokens: 0, totalTokens: 0 },
      agents: [],
      modelStats: [],
      providerModels: [],
    },
    isLoading: false,
    error: null,
  }),
  useRequestLogs: () => ({ data: [], isLoading: false }),
}));

function renderWithQueryClient(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function routerPlan(withV2 = true): Provider {
  return {
    id: "codex-router",
    name: "Codex MultiRouter",
    category: "custom",
    settingsConfig: {
      codexRouting: {
        enabled: true,
        routes: [],
        ...(withV2
          ? {
              subagentV2: {
                schemaVersion: 1,
                selectionPolicy: "balanced",
                profiles: {},
              },
            }
          : {}),
      },
      modelCatalog: {
        models: [{ model: "deepseek-v4-flash", contextWindow: 128000 }],
      },
    },
  };
}

describe("shared Codex Sub-Agent V2 profile editor contract", () => {
  it("exposes the same V2 capability questionnaire from the setup wizard and workspace", async () => {
    const user = userEvent.setup();
    const plan = routerPlan();
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[plan]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Sub-Agent V2/ }));
    expect(
      screen.getByRole("heading", { name: "V2 能力问卷" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("全局选择策略")).toHaveValue("balanced");
    expect(screen.getByLabelText("任务优势")).toBeInTheDocument();
    expect(screen.getByLabelText("优化目标")).toHaveValue("balanced");
    expect(screen.getByLabelText("写入范围")).toHaveValue("bounded_changes");
    expect(screen.getByLabelText("路由偏好")).toHaveValue("eligible");
    expect(screen.getByLabelText("推理强度")).toHaveValue("auto");
    expect(screen.getByLabelText("任务优势")).toHaveAttribute(
      "data-min-selections",
      "1",
    );
    expect(screen.getByLabelText("任务优势")).toHaveAttribute(
      "data-max-selections",
      "5",
    );
    expect(screen.getByLabelText("优化目标")).toHaveTextContent(
      "speedbalancedquality",
    );
    expect(screen.getByLabelText("写入范围")).toHaveTextContent(
      "read_onlybounded_changescomplex_changes",
    );
    expect(screen.getByLabelText("路由偏好")).toHaveTextContent(
      "preferredeligiblefallback",
    );
    expect(screen.getByLabelText("推理强度")).toHaveTextContent(
      "autolowmediumhighxhigh",
    );
  });

  it("initializes legacy plans through the shared editor instead of a client-side schema", async () => {
    const user = userEvent.setup();
    const plan = routerPlan(false);
    renderWithQueryClient(
      <CodexRouterWorkspacePage
        providers={[plan]}
        activeProviderId={plan.id}
        initialProviderId={plan.id}
        initialTab="routes"
        isProxyRunning={false}
        isCodexTakeoverActive={false}
        onEditProvider={vi.fn()}
        onDeletePlan={vi.fn()}
        onCreateProvider={vi.fn()}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "初始化 V2 能力配置" }),
    );
    expect(
      screen.getByRole("heading", { name: "V2 能力问卷" }),
    ).toBeInTheDocument();
  });

  it("renders backend preview fields and sends only settingsConfig, model, and profile over IPC", async () => {
    const user = userEvent.setup();
    const plan = routerPlan();
    renderWithQueryClient(
      <CodexRouterWorkspacePage
        providers={[plan]}
        activeProviderId={plan.id}
        initialProviderId={plan.id}
        initialTab="routes"
        isProxyRunning={false}
        isCodexTakeoverActive={false}
        onEditProvider={vi.fn()}
        onDeletePlan={vi.fn()}
        onCreateProvider={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "编辑 V2 能力配置" }));
    expect(screen.getByText(previewResponse.tomlPreview)).toBeInTheDocument();
    expect(screen.getByText("third_party")).toBeInTheDocument();
    expect(screen.getByText("repo-reader")).toBeInTheDocument();
    expect(screen.getByText("codex_model_router_v2")).toBeInTheDocument();
    expect(screen.getByText("128000")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("preview_codex_subagent_profile", {
      settingsConfig: plan.settingsConfig,
      model: "deepseek-v4-flash",
      profile: {
        model: "deepseek-v4-flash",
        enabled: true,
        questionnaire: {
          taskStrengths: expect.any(Array),
          optimization: expect.any(String),
          writeScope: expect.any(String),
          preference: expect.any(String),
          reasoningEffort: expect.any(String),
        },
      },
    });
  });

  it("keeps manual field overrides independent while regeneration refreshes only generated fields", async () => {
    const user = userEvent.setup();
    const plan = routerPlan();
    renderWithQueryClient(
      <CodexRouterWorkspacePage
        providers={[plan]}
        activeProviderId={plan.id}
        initialProviderId={plan.id}
        initialTab="routes"
        isProxyRunning={false}
        isCodexTakeoverActive={false}
        onEditProvider={vi.fn()}
        onDeletePlan={vi.fn()}
        onCreateProvider={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "编辑 V2 能力配置" }));
    await user.click(screen.getByRole("button", { name: "手动覆盖描述" }));
    await user.type(screen.getByLabelText("描述"), "Manual description");
    await user.click(screen.getByRole("button", { name: "重新生成预览" }));
    expect(screen.getByDisplayValue("Manual description")).toBeInTheDocument();
    expect(screen.getByText("手动覆盖")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "恢复描述" }));
    expect(screen.getByText("自动生成")).toBeInTheDocument();
  });
});
