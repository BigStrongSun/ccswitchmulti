import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { CodexMultiRouterWizard } from "./CodexMultiRouterWizard";
import { CodexRouterWorkspacePage } from "./CodexRouterWorkspacePage";

const previewFixture = {
  providerKind: "third_party" as const,
  requestedRoleName: "repository-scout",
  effectiveRoleName: "repository-scout-2",
  description: "Read the repository and collect evidence.",
  developerInstructions: "Do not modify source files.",
  nicknameCandidates: ["Scout", "Reader"],
  model: "deepseek-v4-flash",
  modelProvider: "codex_model_router_v2",
  modelReasoningEffort: "medium" as const,
  modelContextWindow: 128000,
  tomlPreview:
    '[agents.repository-scout-2]\nmodel = "deepseek-v4-flash"\nmodel_provider = "codex_model_router_v2"',
  warnings: ["Role name was collision-resolved."],
};

const statusFixture = {
  mode: "v2" as const,
  generationSource: "configured_profiles" as const,
  profiles: [
    {
      profileKey: "repository-scout",
      model: "deepseek-v4-flash",
      providerKind: "third_party" as const,
      enabled: true,
      routable: true,
      fieldSources: {
        roleName: "override" as const,
        description: "automatic" as const,
        developerInstructions: "override" as const,
        nicknameCandidates: "automatic" as const,
        modelReasoningEffort: "override" as const,
      },
      requestedRoleName: "repository-scout",
      effectiveRoleName: "repository-scout-2",
      roleFilePath: "C:\\Codex\\agents\\repository-scout-2.toml",
      modelProvider: "codex_model_router_v2" as const,
      modelReasoningEffort: "medium" as const,
      status: "generated" as const,
      warnings: ["Role name was collision-resolved."],
    },
    {
      profileKey: "offline-writer",
      model: "deepseek-v4-pro",
      providerKind: "third_party" as const,
      enabled: true,
      routable: false,
      status: "unroutable" as const,
      nonGenerationReason: "unroutable" as const,
      warnings: ["No enabled route resolves this model."],
    },
  ],
  warnings: ["One profile is unroutable."],
};

// This suite only replaces the Tauri process boundary. Each command returns a
// complete backend DTO; no client-side capability, route, role-path, or status
// compilation is mirrored in a mock.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) => {
    switch (command) {
      case "preview_codex_subagent_profile":
        return Promise.resolve(previewFixture);
      case "get_codex_subagent_profile_statuses":
        return Promise.resolve(statusFixture);
      case "get_global_proxy_config":
        return Promise.resolve({
          listenAddress: "127.0.0.1",
          listenPort: 15721,
        });
      case "get_codex_account_pool_policy":
        return Promise.resolve({ enabled: false, entries: [] });
      case "get_codex_oauth_models":
        return Promise.resolve([]);
      default:
        return Promise.resolve(true);
    }
  }),
}));

function plan(withV2 = true): Provider {
  return {
    id: "router",
    name: "Codex MultiRouter",
    category: "custom",
    settingsConfig: {
      codexRouting: {
        enabled: true,
        routes: [
          {
            id: "flash-route",
            enabled: true,
            targetProviderId: "third-party",
            match: { models: ["deepseek-v4-flash"], prefixes: [] },
            upstream: {},
          },
        ],
        ...(withV2
          ? {
              subagentV2: {
                schemaVersion: 1,
                selectionPolicy: "balanced",
                profiles: {
                  "repository-scout": {
                    model: "deepseek-v4-flash",
                    enabled: true,
                    questionnaire: {
                      taskStrengths: ["repository_exploration"],
                      optimization: "balanced",
                      writeScope: "read_only",
                      preference: "eligible",
                      reasoningEffort: "auto",
                    },
                    overrides: {
                      roleName: "repository-scout",
                      developerInstructions: "Do not modify source files.",
                      modelReasoningEffort: "medium",
                    },
                  },
                  "offline-writer": {
                    model: "deepseek-v4-pro",
                    enabled: true,
                    questionnaire: {
                      taskStrengths: ["complex_implementation"],
                      optimization: "quality",
                      writeScope: "complex_changes",
                      preference: "fallback",
                      reasoningEffort: "high",
                    },
                  },
                },
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

function provider(): Provider {
  return {
    id: "third-party",
    name: "DeepSeek",
    category: "custom",
    settingsConfig: {
      baseUrl: "https://example.invalid/v1",
      auth: { OPENAI_API_KEY: "red-test-only" },
      modelCatalog: {
        models: [{ model: "deepseek-v4-flash", contextWindow: 128000 }],
      },
    },
  };
}

function renderWorkspace(withV2 = true) {
  const selectedPlan = plan(withV2);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <CodexRouterWorkspacePage
        providers={[provider(), selectedPlan]}
        activeProviderId={selectedPlan.id}
        initialProviderId={selectedPlan.id}
        initialTab="routes"
        isProxyRunning={false}
        isCodexTakeoverActive={false}
        onEditProvider={vi.fn()}
        onDeletePlan={vi.fn()}
        onCreateProvider={vi.fn()}
      />
    </QueryClientProvider>,
  );
  return selectedPlan;
}

function renderWizard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <CodexMultiRouterWizard
        open
        providers={[provider(), plan()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

afterEach(() => vi.mocked(invoke).mockClear());

describe("Codex Sub-Agent V2 shared profile editor RED contract", () => {
  it("wizard exposes the shared selection policy control", () => {
    renderWizard();
    expect(screen.getByLabelText("全局选择策略")).toHaveValue("balanced");
  });

  it("workspace exposes the same selection policy control", () => {
    renderWorkspace();
    expect(screen.getByLabelText("全局选择策略")).toHaveValue("balanced");
  });

  it("limits unique task strengths to one through five selections", () => {
    renderWorkspace();
    expect(screen.getByLabelText("任务优势")).toHaveAttribute(
      "data-selection-range",
      "1-5-unique",
    );
  });

  it("offers every optimization enum", () => {
    renderWorkspace();
    expect(screen.getByLabelText("优化目标")).toHaveTextContent(
      "speedbalancedquality",
    );
  });

  it("offers every write scope enum", () => {
    renderWorkspace();
    expect(screen.getByLabelText("写入范围")).toHaveTextContent(
      "read_onlybounded_changescomplex_changes",
    );
  });

  it("offers every route preference enum", () => {
    renderWorkspace();
    expect(screen.getByLabelText("路由偏好")).toHaveTextContent(
      "preferredeligiblefallback",
    );
  });

  it("offers every reasoning effort enum", () => {
    renderWorkspace();
    expect(screen.getByLabelText("推理强度")).toHaveTextContent(
      "autolowmediumhighxhigh",
    );
  });

  it("shows enabled state without treating catalog presence as routability", () => {
    renderWorkspace();
    expect(screen.getByText("已启用且可路由")).toBeInTheDocument();
  });

  it("shows the unroutable profile as a controlled non-generation state", () => {
    renderWorkspace();
    expect(screen.getByText("unroutable")).toBeInTheDocument();
  });

  it("offers one-click legacy V2 initialization", () => {
    renderWorkspace(false);
    expect(
      screen.getByRole("button", { name: "初始化 V2 能力配置" }),
    ).toBeInTheDocument();
  });

  it("renders backend TOML rather than compiling a browser preview", async () => {
    renderWorkspace();
    expect(
      await screen.findByText(previewFixture.tomlPreview),
    ).toBeInTheDocument();
  });

  it("renders requested and collision-resolved effective role names independently", () => {
    renderWorkspace();
    expect(screen.getByText("repository-scout-2")).toBeInTheDocument();
  });

  it("renders backend description as the final field value", async () => {
    renderWorkspace();
    expect(
      await screen.findByText(previewFixture.description),
    ).toBeInTheDocument();
  });

  it("renders backend developer instructions as the final field value", async () => {
    renderWorkspace();
    expect(
      await screen.findByText(previewFixture.developerInstructions),
    ).toBeInTheDocument();
  });

  it("renders backend nickname candidates as final fields", () => {
    renderWorkspace();
    expect(screen.getByText("Scout")).toBeInTheDocument();
  });

  it("renders backend reasoning, provider, context, and warning", () => {
    renderWorkspace();
    expect(screen.getByText("medium")).toBeInTheDocument();
  });

  it("requests preview with the exact public settingsConfig, model, and profile payload", async () => {
    const selectedPlan = renderWorkspace();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("preview_codex_subagent_profile", {
        settingsConfig: selectedPlan.settingsConfig,
        model: "deepseek-v4-flash",
        profile: {
          model: "deepseek-v4-flash",
          enabled: true,
          questionnaire: {
            taskStrengths: ["repository_exploration"],
            optimization: "balanced",
            writeScope: "read_only",
            preference: "eligible",
            reasoningEffort: "auto",
          },
          overrides: {
            roleName: "repository-scout",
            developerInstructions: "Do not modify source files.",
            modelReasoningEffort: "medium",
          },
        },
      }),
    );
  });

  it("requests authoritative statuses with only settingsConfig", async () => {
    const selectedPlan = renderWorkspace();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "get_codex_subagent_profile_statuses",
        {
          settingsConfig: selectedPlan.settingsConfig,
        },
      ),
    );
  });

  it("renders status provider kind, field sources, role path, and generation source", () => {
    renderWorkspace();
    expect(screen.getByText("configured_profiles")).toBeInTheDocument();
  });

  it("preserves a manual description when questionnaire answers change", async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole("button", { name: "手动覆盖描述" }));
    await user.type(screen.getByLabelText("描述"), "Manual description");
    await user.selectOptions(screen.getByLabelText("优化目标"), "quality");
    expect(screen.getByDisplayValue("Manual description")).toBeInTheDocument();
  });

  it("restores only the description override", async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole("button", { name: "恢复描述" }));
    expect(screen.getByText("自动生成")).toBeInTheDocument();
    expect(screen.getByText("角色名：手动覆盖")).toBeInTheDocument();
  });

  it("keeps role-name override independent from other manual fields", async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole("button", { name: "手动覆盖角色名" }));
    await user.type(screen.getByLabelText("角色名"), "Manual role");
    expect(screen.getByDisplayValue("Manual role")).toBeInTheDocument();
  });

  it("keeps developer-instructions override independent from other manual fields", async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(
      screen.getByRole("button", { name: "手动覆盖开发者指令" }),
    );
    await user.type(screen.getByLabelText("开发者指令"), "Manual instructions");
    expect(screen.getByDisplayValue("Manual instructions")).toBeInTheDocument();
  });

  it("keeps nickname-candidates override independent from other manual fields", async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(screen.getByRole("button", { name: "手动覆盖昵称候选" }));
    await user.type(screen.getByLabelText("昵称候选"), "Manual nickname");
    expect(screen.getByDisplayValue("Manual nickname")).toBeInTheDocument();
  });

  it("keeps model-reasoning override independent from other manual fields", async () => {
    const user = userEvent.setup();
    renderWorkspace();
    await user.click(
      screen.getByRole("button", { name: "手动覆盖模型推理强度" }),
    );
    await user.selectOptions(screen.getByLabelText("模型推理强度"), "high");
    expect(screen.getByLabelText("模型推理强度")).toHaveValue("high");
  });

  it("refreshes the persisted V2 fields after saving and remounting", () => {
    renderWorkspace();
    expect(
      screen.getByRole("button", { name: "保存 V2 能力配置" }),
    ).toBeInTheDocument();
  });
});
