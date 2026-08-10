import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  getDefaultNormalizer,
  render,
  screen,
  waitFor,
  within,
  type RenderResult,
} from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { providersApi } from "@/lib/api/providers";
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

const ipcState = vi.hoisted(() => ({
  providers: {} as Record<string, Provider>,
}));

// The only test double is the real frontend process boundary. The dispatcher
// keeps the same provider record that get_providers/update_provider use in the
// application, so save/remount tests cannot pass through a second local schema.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case "get_providers":
        return JSON.parse(JSON.stringify(ipcState.providers));
      case "update_provider": {
        if (!args || args.app !== "codex") {
          throw new Error(
            'update_provider must receive the source envelope app="codex"',
          );
        }
        if (!Object.prototype.hasOwnProperty.call(args, "originalId")) {
          throw new Error(
            "update_provider must include originalId in the source envelope",
          );
        }
        if (args.originalId !== undefined) {
          throw new Error(
            "update_provider stable-plan save expects originalId to be undefined",
          );
        }
        const savedProvider = args?.provider as Provider;
        if (!savedProvider || typeof savedProvider.id !== "string") {
          throw new Error(
            "update_provider must include a provider with a stable id",
          );
        }
        ipcState.providers[savedProvider.id] = JSON.parse(
          JSON.stringify(savedProvider),
        );
        return true;
      }
      case "add_provider": {
        const savedProvider = args?.provider as Provider;
        ipcState.providers[savedProvider.id] = JSON.parse(
          JSON.stringify(savedProvider),
        );
        return true;
      }
      case "preview_codex_subagent_profile":
        return JSON.parse(JSON.stringify(previewFixture));
      case "get_codex_subagent_profile_statuses":
        return JSON.parse(JSON.stringify(statusFixture));
      case "get_global_proxy_config":
        return {
          listenAddress: "127.0.0.1",
          listenPort: 15721,
        };
      case "get_codex_account_pool_policy":
        return { enabled: false, entries: [], desktopAccountId: null };
      case "auth_get_status":
        return {
          provider: "codex_oauth",
          authenticated: false,
          default_account_id: null,
          migration_error: null,
          auth_error: null,
          accounts: [],
        };
      case "get_codex_oauth_models":
        return [];
      default:
        throw new Error(
          `Unexpected Tauri command in V2 editor test: ${command}`,
        );
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
        models: [
          { model: "deepseek-v4-flash", contextWindow: 128000 },
          { model: "deepseek-v4-pro", contextWindow: 128000 },
        ],
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
      // Deliberately no key: the real workspace settles in its controlled
      // "cannot refresh /models" state instead of starting unrelated writes.
      auth: {},
      modelCatalog: {
        models: [
          { model: "deepseek-v4-flash", contextWindow: 128000 },
          { model: "deepseek-v4-pro", contextWindow: 128000 },
        ],
      },
    },
  };
}

function seedPersistedPlan(withV2 = true) {
  const source = provider();
  const persistedPlan = plan(withV2);
  ipcState.providers = {
    [source.id]: source,
    [persistedPlan.id]: persistedPlan,
  };
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
}

async function mountWorkspaceFromPersistedPlan(): Promise<
  RenderResult & { selectedPlan: Provider }
> {
  const loaded = await providersApi.getAll("codex");
  const selectedPlan = loaded.router;
  const queryClient = createQueryClient();
  const result = render(
    <QueryClientProvider client={queryClient}>
      <CodexRouterWorkspacePage
        providers={Object.values(loaded)}
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
  await screen.findByRole("tab", { name: "路由规则" });
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("get_global_proxy_config"),
  );
  return { ...result, selectedPlan };
}

async function renderWorkspace(withV2 = true) {
  seedPersistedPlan(withV2);
  return mountWorkspaceFromPersistedPlan();
}

async function mountWizardFromPersistedPlan() {
  const loaded = await providersApi.getAll("codex");
  const queryClient = createQueryClient();
  const result = render(
    <QueryClientProvider client={queryClient}>
      <CodexMultiRouterWizard
        open
        providers={Object.values(loaded)}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("auth_get_status", {
      authProvider: "codex_oauth",
    }),
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /Sub-Agent V2/ }));
  return { ...result, user };
}

async function renderWizard() {
  seedPersistedPlan(true);
  return mountWizardFromPersistedPlan();
}

function updateProviderCalls() {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "update_provider");
}

function latestSavedPlan() {
  const call = updateProviderCalls().at(-1);
  return (call?.[1] as { provider?: Provider } | undefined)?.provider;
}

async function saveV2(user: UserEvent) {
  await user.click(
    await screen.findByRole("button", {
      name: "保存 V2 子 Agent 能力配置",
    }),
  );
}

async function chooseOption(
  user: UserEvent,
  control: HTMLElement,
  optionName: string,
) {
  if (control instanceof HTMLSelectElement) {
    await user.selectOptions(control, optionName);
    return;
  }
  await user.click(control);
  await user.click(await screen.findByRole("option", { name: optionName }));
}

function flashRegion() {
  return screen.getByRole("region", {
    name: "DeepSeek V4 Flash 子 Agent 能力",
  });
}

function expectControlValue(
  control: HTMLElement,
  persistedValue: string,
  visibleValue = persistedValue,
) {
  if (control instanceof HTMLSelectElement) {
    expect(control).toHaveValue(persistedValue);
  } else {
    expect(control).toHaveTextContent(visibleValue);
  }
}

async function expectSavedSettingsConfig(expected: Record<string, unknown>) {
  await waitFor(() =>
    expect(latestSavedPlan()?.settingsConfig).toEqual(expected),
  );
}

async function expectSavedSubagentV2(expected: Record<string, unknown>) {
  await waitFor(() =>
    expect(latestSavedPlan()?.settingsConfig?.codexRouting?.subagentV2).toEqual(
      expected,
    ),
  );
}

beforeEach(() => {
  seedPersistedPlan(true);
  vi.mocked(invoke).mockClear();
});

describe("Codex Sub-Agent V2 shared editor accessible areas", () => {
  it.each(["选择策略", "模型能力问卷", "最终字段", "TOML 预览"])(
    "renders the %s area as its own heading",
    async (heading) => {
      await renderWorkspace();
      expect(
        await screen.findByRole("heading", { name: heading }),
      ).toBeInTheDocument();
    },
  );
});

describe("Codex Sub-Agent V2 persisted interactions", () => {
  it.each([
    ["均衡", "balanced"],
    ["官方优先", "official_first"],
    ["第三方优先", "third_party_first"],
  ])(
    "saves policy option %s as %s through update_provider",
    async (optionName, persistedValue) => {
      const user = userEvent.setup();
      await renderWorkspace();
      await chooseOption(
        user,
        await screen.findByLabelText("第三方子 Agent 选择策略"),
        optionName,
      );
      await saveV2(user);
      await expectSavedSettingsConfig({
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
          subagentV2: {
            schemaVersion: 1,
            selectionPolicy: persistedValue,
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
        },
        modelCatalog: {
          models: [
            { model: "deepseek-v4-flash", contextWindow: 128000 },
            { model: "deepseek-v4-pro", contextWindow: 128000 },
          ],
        },
      });
    },
  );

  it("changes only the selected model enabled flag in the full save payload", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await user.click(
      within(flashRegion()).getByLabelText("启用此模型作为 V2 子 Agent"),
    );
    await saveV2(user);
    await expectSavedSettingsConfig({
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
        subagentV2: {
          schemaVersion: 1,
          selectionPolicy: "balanced",
          profiles: {
            "repository-scout": {
              model: "deepseek-v4-flash",
              enabled: false,
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
      },
      modelCatalog: {
        models: [
          { model: "deepseek-v4-flash", contextWindow: 128000 },
          { model: "deepseek-v4-pro", contextWindow: 128000 },
        ],
      },
    });
  });

  it("saves exactly one strength selected through a real checkbox", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const strengths = within(
      within(flashRegion()).getByRole("group", { name: "任务优势" }),
    );
    await user.click(strengths.getByDisplayValue("repository_exploration"));
    await user.click(strengths.getByDisplayValue("testing"));
    await saveV2(user);
    await expectSavedSubagentV2({
      schemaVersion: 1,
      selectionPolicy: "balanced",
      profiles: {
        "repository-scout": {
          model: "deepseek-v4-flash",
          enabled: true,
          questionnaire: {
            taskStrengths: ["testing"],
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
    });
  });

  it("saves five unique strengths in stable click order", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const strengths = within(
      within(flashRegion()).getByRole("group", { name: "任务优势" }),
    );
    for (const value of [
      "long_context_reading",
      "evidence_collection",
      "summarization",
      "testing",
    ]) {
      await user.click(strengths.getByDisplayValue(value));
    }
    await saveV2(user);
    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].questionnaire.taskStrengths,
      ).toEqual([
        "repository_exploration",
        "long_context_reading",
        "evidence_collection",
        "summarization",
        "testing",
      ]),
    );
  });

  it("does not duplicate a strength after deselect and reselect", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const checkbox = within(
      within(flashRegion()).getByRole("group", { name: "任务优势" }),
    ).getByDisplayValue("repository_exploration");
    await user.click(checkbox);
    await user.click(checkbox);
    await user.click(checkbox);
    await user.click(checkbox);
    await saveV2(user);
    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].questionnaire.taskStrengths,
      ).toEqual(["repository_exploration"]),
    );
  });

  it("rejects a sixth strength with a visible limit and preserves the first five", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const strengths = within(
      within(flashRegion()).getByRole("group", { name: "任务优势" }),
    );
    for (const value of [
      "long_context_reading",
      "evidence_collection",
      "summarization",
      "testing",
    ]) {
      await user.click(strengths.getByDisplayValue(value));
    }
    await user.click(strengths.getByDisplayValue("high_risk_review"));
    expect(
      await screen.findByText("任务优势最多选择 5 项"),
    ).toBeInTheDocument();
    await saveV2(user);
    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].questionnaire.taskStrengths,
      ).toEqual([
        "repository_exploration",
        "long_context_reading",
        "evidence_collection",
        "summarization",
        "testing",
      ]),
    );
  });

  it.each([
    [
      "优化目标",
      "speed",
      "quality",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "speed",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "优化目标",
      "balanced",
      "speed",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "优化目标",
      "quality",
      "speed",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "quality",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "写入范围",
      "read_only",
      "bounded_changes",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "写入范围",
      "bounded_changes",
      "read_only",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "bounded_changes",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "写入范围",
      "complex_changes",
      "read_only",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "complex_changes",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "模型偏好",
      "preferred",
      "fallback",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "preferred",
        reasoningEffort: "auto",
      },
    ],
    [
      "模型偏好",
      "eligible",
      "preferred",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "模型偏好",
      "fallback",
      "preferred",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "fallback",
        reasoningEffort: "auto",
      },
    ],
    [
      "推理强度",
      "auto",
      "low",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "auto",
      },
    ],
    [
      "推理强度",
      "low",
      "medium",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "low",
      },
    ],
    [
      "推理强度",
      "medium",
      "low",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "medium",
      },
    ],
    [
      "推理强度",
      "high",
      "low",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "high",
      },
    ],
    [
      "推理强度",
      "xhigh",
      "low",
      {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
        reasoningEffort: "xhigh",
      },
    ],
  ])(
    "saves %s=%s without changing questionnaire siblings",
    async (label, value, alternate, expectedQuestionnaire) => {
      const user = userEvent.setup();
      await renderWorkspace();
      const control = within(flashRegion()).getByLabelText(label);
      if (
        (control instanceof HTMLSelectElement && control.value === value) ||
        (!(control instanceof HTMLSelectElement) &&
          control.textContent?.includes(value))
      ) {
        await chooseOption(user, control, alternate);
      }
      await chooseOption(user, control, value);
      await saveV2(user);
      await waitFor(() =>
        expect(
          latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
            "repository-scout"
          ].questionnaire,
        ).toEqual(expectedQuestionnaire),
      );
    },
  );

  it.each([
    ["角色名称", "audit-scout", "roleName", "audit-scout"],
    [
      "角色描述",
      "Audit repositories and preserve evidence.",
      "description",
      "Audit repositories and preserve evidence.",
    ],
    [
      "开发者指令",
      "Inspect deeply and never write source files.",
      "developerInstructions",
      "Inspect deeply and never write source files.",
    ],
    [
      "昵称候选",
      "Navigator, Reviewer",
      "nicknameCandidates",
      ["Navigator", "Reviewer"],
    ],
  ])(
    "edits %s and persists only its override while preserving the others",
    async (label, typedValue, overrideKey, persistedValue) => {
      const user = userEvent.setup();
      await renderWorkspace();
      const input = within(flashRegion()).getByLabelText(label);
      await user.clear(input);
      await user.type(input, typedValue);
      await saveV2(user);
      await waitFor(() =>
        expect(
          latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
            "repository-scout"
          ].overrides,
        ).toEqual({
          roleName:
            overrideKey === "roleName" ? persistedValue : "repository-scout",
          ...(overrideKey === "description"
            ? { description: persistedValue }
            : {}),
          developerInstructions:
            overrideKey === "developerInstructions"
              ? persistedValue
              : "Do not modify source files.",
          ...(overrideKey === "nicknameCandidates"
            ? { nicknameCandidates: persistedValue }
            : {}),
          modelReasoningEffort: "medium",
        }),
      );
    },
  );

  it("edits model reasoning effort and preserves all other overrides", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("模型推理强度"),
      "high",
    );
    await saveV2(user);
    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].overrides,
      ).toEqual({
        roleName: "repository-scout",
        developerInstructions: "Do not modify source files.",
        modelReasoningEffort: "high",
      }),
    );
  });

  it("removes description alone after all five overrides were established", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const region = within(flashRegion());
    for (const [label, value] of [
      ["角色名称", "audit-role"],
      ["角色描述", "Manual description to restore"],
      ["开发者指令", "Audit without modifying source files."],
      ["昵称候选", "Audit, Reviewer"],
    ]) {
      const input = region.getByLabelText(label);
      await user.clear(input);
      await user.type(input, value);
    }
    await chooseOption(user, region.getByLabelText("模型推理强度"), "high");
    await user.click(
      region.getByRole("button", { name: "恢复角色描述自动值" }),
    );
    await saveV2(user);
    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].overrides,
      ).toEqual({
        roleName: "audit-role",
        developerInstructions: "Audit without modifying source files.",
        nicknameCandidates: ["Audit", "Reviewer"],
        modelReasoningEffort: "high",
      }),
    );
  });

  it("reloads policy, questionnaire, and override from get_providers after save/remount", async () => {
    const user = userEvent.setup();
    const firstMount = await renderWorkspace();
    await chooseOption(
      user,
      await screen.findByLabelText("第三方子 Agent 选择策略"),
      "第三方优先",
    );
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("优化目标"),
      "quality",
    );
    const description = within(flashRegion()).getByLabelText("角色描述");
    await user.clear(description);
    await user.type(description, "Persisted manual description");
    await saveV2(user);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_provider", {
        provider: {
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
                  match: {
                    models: ["deepseek-v4-flash"],
                    prefixes: [],
                  },
                  upstream: {},
                },
              ],
              subagentV2: {
                schemaVersion: 1,
                selectionPolicy: "third_party_first",
                profiles: {
                  "repository-scout": {
                    model: "deepseek-v4-flash",
                    enabled: true,
                    questionnaire: {
                      taskStrengths: ["repository_exploration"],
                      optimization: "quality",
                      writeScope: "read_only",
                      preference: "eligible",
                      reasoningEffort: "auto",
                    },
                    overrides: {
                      roleName: "repository-scout",
                      description: "Persisted manual description",
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
            },
            modelCatalog: {
              models: [
                {
                  model: "deepseek-v4-flash",
                  contextWindow: 128000,
                },
                { model: "deepseek-v4-pro", contextWindow: 128000 },
              ],
            },
          },
        },
        app: "codex",
        originalId: undefined,
      }),
    );
    expect(updateProviderCalls()).toHaveLength(1);
    firstMount.unmount();

    await mountWorkspaceFromPersistedPlan();
    expectControlValue(
      await screen.findByLabelText("第三方子 Agent 选择策略"),
      "third_party_first",
      "第三方优先",
    );
    expectControlValue(
      within(flashRegion()).getByLabelText("优化目标"),
      "quality",
    );
    expect(within(flashRegion()).getByLabelText("角色描述")).toHaveValue(
      "Persisted manual description",
    );
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === "get_providers"),
    ).toHaveLength(2);
  });

  it("shares the wizard-saved V2 source with the remounted workspace", async () => {
    const wizard = await renderWizard();
    await chooseOption(
      wizard.user,
      await screen.findByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );
    await chooseOption(
      wizard.user,
      within(flashRegion()).getByLabelText("模型偏好"),
      "preferred",
    );
    const roleName = within(flashRegion()).getByLabelText("角色名称");
    await wizard.user.clear(roleName);
    await wizard.user.type(roleName, "wizard-scout");
    await saveV2(wizard.user);
    await waitFor(() => expect(updateProviderCalls()).toHaveLength(1));
    wizard.unmount();

    await mountWorkspaceFromPersistedPlan();
    expectControlValue(
      await screen.findByLabelText("第三方子 Agent 选择策略"),
      "official_first",
      "官方优先",
    );
    expectControlValue(
      within(flashRegion()).getByLabelText("模型偏好"),
      "preferred",
    );
    expect(within(flashRegion()).getByLabelText("角色名称")).toHaveValue(
      "wizard-scout",
    );
  });

  it("initializes the exact legacy V2 defaults with one update_provider write", async () => {
    const user = userEvent.setup();
    await renderWorkspace(false);
    await user.click(
      await screen.findByRole("button", {
        name: "初始化 V2 子 Agent 能力配置",
      }),
    );
    await waitFor(() => expect(updateProviderCalls()).toHaveLength(1));
    await expectSavedSubagentV2({
      schemaVersion: 1,
      selectionPolicy: "balanced",
      profiles: {
        "deepseek-v4-flash": {
          model: "deepseek-v4-flash",
          enabled: true,
          questionnaire: {
            taskStrengths: [
              "long_context_reading",
              "repository_exploration",
              "evidence_collection",
              "summarization",
              "testing",
            ],
            optimization: "speed",
            writeScope: "read_only",
            preference: "eligible",
            reasoningEffort: "medium",
          },
        },
        "deepseek-v4-pro": {
          model: "deepseek-v4-pro",
          enabled: true,
          questionnaire: {
            taskStrengths: [
              "complex_debugging",
              "architecture_design",
              "complex_implementation",
              "high_risk_review",
              "testing",
            ],
            optimization: "quality",
            writeScope: "complex_changes",
            preference: "eligible",
            reasoningEffort: "high",
          },
        },
      },
    });
  });
});

describe("Codex Sub-Agent V2 preview visible output", () => {
  it("distinguishes requested and effective role names", async () => {
    await renderWorkspace();
    expect(await screen.findByText("请求角色名")).toBeInTheDocument();
    expect(screen.getByText("repository-scout")).toBeInTheDocument();
    expect(screen.getByText("实际角色名")).toBeInTheDocument();
    expect(screen.getByText("repository-scout-2")).toBeInTheDocument();
  });

  it.each([
    ["description", previewFixture.description],
    ["developer instructions", previewFixture.developerInstructions],
    ["first nickname candidate", "Scout"],
    ["second nickname candidate", "Reader"],
    ["fixed model provider", "codex_model_router_v2"],
    ["model reasoning effort", "medium"],
    ["context window", "128000"],
    ["warning", "Role name was collision-resolved."],
  ])("renders backend-returned %s", async (_field, value) => {
    await renderWorkspace();
    expect(await screen.findByText(value)).toBeInTheDocument();
  });

  it("renders backend-returned backend TOML", async () => {
    await renderWorkspace();
    expect(
      await screen.findByText(previewFixture.tomlPreview, {
        normalizer: getDefaultNormalizer({
          trim: false,
          collapseWhitespace: false,
        }),
      }),
    ).toBeInTheDocument();
  });

  it("requests preview with exact settingsConfig, model, and profile", async () => {
    const { selectedPlan } = await renderWorkspace();
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
});

describe("Codex Sub-Agent V2 authoritative status visible output", () => {
  it.each([
    ["provider kind", "Provider 类型：third_party"],
    ["routable flag", "可路由：是"],
    ["enabled flag", "已启用：是"],
    ["role-name source", "角色名称来源：override"],
    ["description source", "角色描述来源：automatic"],
    ["developer-instructions source", "开发者指令来源：override"],
    ["nickname-candidates source", "昵称候选来源：automatic"],
    ["model-reasoning source", "模型推理强度来源：override"],
    ["absolute role path", "C:\\Codex\\agents\\repository-scout-2.toml"],
    ["model", "模型：deepseek-v4-flash"],
    ["model provider", "模型 Provider：codex_model_router_v2"],
    ["reasoning effort", "推理强度：medium"],
    ["generation source", "生成来源：configured_profiles"],
    ["generated status", "生成状态：generated"],
    ["second profile status", "offline-writer：unroutable"],
    ["second profile controlled reason", "未生成原因：unroutable"],
  ])("renders authoritative %s", async (_field, value) => {
    await renderWorkspace();
    expect(await screen.findByText(value)).toBeInTheDocument();
  });

  it("renders authoritative requested and effective roles separately", async () => {
    await renderWorkspace();
    expect(await screen.findByText("请求角色名")).toBeInTheDocument();
    expect(screen.getByText("repository-scout")).toBeInTheDocument();
    expect(screen.getByText("实际角色名")).toBeInTheDocument();
    expect(screen.getByText("repository-scout-2")).toBeInTheDocument();
  });

  it("requests authoritative statuses with the exact settingsConfig-only payload", async () => {
    const { selectedPlan } = await renderWorkspace();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "get_codex_subagent_profile_statuses",
        { settingsConfig: selectedPlan.settingsConfig },
      ),
    );
  });
});
