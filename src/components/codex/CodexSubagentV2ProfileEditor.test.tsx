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
import type { CodexSubagentProfilePreview } from "@/types/codexSubagentV2";
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
} satisfies CodexSubagentProfilePreview;

const proPreviewFixture = {
  providerKind: "third_party" as const,
  requestedRoleName: "deep-reviewer",
  effectiveRoleName: "deep-reviewer",
  description: "Trace cross-module failures and review risky changes.",
  developerInstructions: "Investigate root causes before editing files.",
  nicknameCandidates: ["Reviewer", "Debugger"],
  model: "deepseek-v4-pro",
  modelProvider: "codex_model_router_v2" as const,
  modelReasoningEffort: "high" as const,
  modelContextWindow: 256000,
  tomlPreview:
    '[agents.deep-reviewer]\nmodel = "deepseek-v4-pro"\nmodel_provider = "codex_model_router_v2"',
  warnings: ["Pro profile warning."],
} satisfies CodexSubagentProfilePreview;

const previewFixturesByModel: Record<string, CodexSubagentProfilePreview> = {
  "deepseek-v4-flash": previewFixture,
  "deepseek-v4-pro": proPreviewFixture,
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
      requestedRoleName: "deep-reviewer",
      effectiveRoleName: "deep-reviewer",
      status: "unroutable" as const,
      nonGenerationReason: "unroutable" as const,
      warnings: ["No enabled route resolves this model."],
    },
  ],
  warnings: ["One profile is unroutable."],
};

const ipcState = vi.hoisted(() => ({
  providers: {} as Record<string, Provider>,
  previewErrors: {} as Record<string, string>,
  statusResponse: undefined as unknown,
  statusError: null as string | null,
  updateGate: null as Promise<void> | null,
  beforeV2Persistence: null as (() => void) | null,
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
        if (ipcState.updateGate) await ipcState.updateGate;
        ipcState.beforeV2Persistence?.();
        ipcState.providers[savedProvider.id] = JSON.parse(
          JSON.stringify(savedProvider),
        );
        return true;
      }
      case "update_codex_subagent_v2": {
        if (!args || typeof args.providerId !== "string") {
          throw new Error(
            "update_codex_subagent_v2 requires a stable providerId",
          );
        }
        if (!Object.prototype.hasOwnProperty.call(args, "subagentV2")) {
          throw new Error(
            "update_codex_subagent_v2 requires the focused subagentV2 field",
          );
        }
        if (ipcState.updateGate) await ipcState.updateGate;
        ipcState.beforeV2Persistence?.();
        const latest = ipcState.providers[args.providerId];
        if (!latest) throw new Error("provider not found");
        const savedProvider = JSON.parse(JSON.stringify(latest)) as Provider;
        savedProvider.settingsConfig.codexRouting = {
          ...savedProvider.settingsConfig.codexRouting,
          subagentV2: JSON.parse(JSON.stringify(args.subagentV2)),
        };
        ipcState.providers[savedProvider.id] = savedProvider;
        return JSON.parse(JSON.stringify(savedProvider));
      }
      case "add_provider": {
        const savedProvider = args?.provider as Provider;
        ipcState.providers[savedProvider.id] = JSON.parse(
          JSON.stringify(savedProvider),
        );
        return true;
      }
      case "preview_codex_subagent_profile": {
        const model = args?.model;
        if (typeof model !== "string" || !model) {
          throw new Error("preview fixture requires a nonempty model");
        }
        if (ipcState.previewErrors[model]) {
          throw new Error(ipcState.previewErrors[model]);
        }
        const fixture = previewFixturesByModel[model];
        if (!fixture) {
          throw new Error(`No preview fixture registered for model: ${model}`);
        }
        return JSON.parse(JSON.stringify(fixture));
      }
      case "get_codex_subagent_profile_statuses":
        if (ipcState.statusError) throw new Error(ipcState.statusError);
        return JSON.parse(JSON.stringify(ipcState.statusResponse));
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

async function mountWorkspaceWithoutPlan() {
  const source = provider();
  ipcState.providers = { [source.id]: source };
  const loaded = await providersApi.getAll("codex");
  const queryClient = createQueryClient();
  const result = render(
    <QueryClientProvider client={queryClient}>
      <CodexRouterWorkspacePage
        providers={Object.values(loaded)}
        activeProviderId={source.id}
        initialProviderId={source.id}
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
  return { ...result, source };
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

function v2PersistenceCalls() {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "update_codex_subagent_v2");
}

function addProviderCalls() {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "add_provider");
}

function latestSavedPlan() {
  if (v2PersistenceCalls().length > 0) {
    return ipcState.providers.router;
  }
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

function proRegion() {
  return screen.getByRole("region", {
    name: "DeepSeek V4 Pro 子 Agent 能力",
  });
}

function flashBackendRegion() {
  return screen.getByRole("region", {
    name: "repository-scout 后端预览状态",
  });
}

function proBackendRegion() {
  return screen.getByRole("region", {
    name: "offline-writer 后端预览状态",
  });
}

function createDeferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
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
  ipcState.previewErrors = {};
  ipcState.statusResponse = JSON.parse(JSON.stringify(statusFixture));
  ipcState.statusError = null;
  ipcState.updateGate = null;
  ipcState.beforeV2Persistence = null;
  vi.mocked(invoke).mockClear();
});

describe("Codex Sub-Agent V2 review round 1 regressions", () => {
  it("preserves explicit V2 data and unknown routing fields through wizard publish", async () => {
    const originalV2 = ipcState.providers.router.settingsConfig.codexRouting
      .subagentV2 as Record<string, unknown>;
    ipcState.providers.router.settingsConfig.codexRouting.futureRouting = {
      mode: "keep-me",
    };

    const wizard = await mountWizardFromPersistedPlan();
    await wizard.user.click(screen.getByRole("button", { name: "保存并发布" }));
    await wizard.user.click(
      screen.getAllByRole("button", { name: "保存并发布" }).at(-1)!,
    );
    await waitFor(() => expect(updateProviderCalls()).toHaveLength(1));

    expect(latestSavedPlan()?.settingsConfig.codexRouting).toEqual(
      expect.objectContaining({
        futureRouting: { mode: "keep-me" },
        subagentV2: originalV2,
      }),
    );
  });

  it("uses the current wizard plan draft for V2 preview and status requests", async () => {
    await mountWizardFromPersistedPlan();

    await waitFor(() => {
      const statusCalls = vi
        .mocked(invoke)
        .mock.calls.filter(
          ([command]) => command === "get_codex_subagent_profile_statuses",
        );
      expect(statusCalls).toContainEqual([
        "get_codex_subagent_profile_statuses",
        {
          settingsConfig: expect.objectContaining({
            codexRouting: expect.objectContaining({
              routes: expect.arrayContaining([
                expect.objectContaining({
                  match: expect.objectContaining({
                    models: expect.arrayContaining(["deepseek-v4-pro"]),
                  }),
                }),
              ]),
            }),
          }),
        },
      ]);
    });
  });

  it("blocks persistence when authoritative status reports a role collision", async () => {
    ipcState.statusResponse = {
      ...statusFixture,
      profiles: [
        {
          ...statusFixture.profiles[0],
          routable: false,
          status: "collision",
          nonGenerationReason: "collision",
          warnings: ["Normalized role name collision."],
        },
        statusFixture.profiles[1],
      ],
    };
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();
    const saveButton = await screen.findByRole("button", {
      name: "保存 V2 子 Agent 能力配置",
    });

    await user.click(saveButton);
    await waitFor(() => expect(saveButton).toBeEnabled());

    expect(updateProviderCalls()).toHaveLength(0);
    expect(screen.getByRole("alert")).toHaveTextContent(/collision/i);
  });

  it("allows saving while an enabled retained profile is authoritatively unroutable", async () => {
    ipcState.previewErrors["deepseek-v4-pro"] =
      "No enabled route resolves this model.";
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    await saveV2(user);

    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
        "offline-writer"
      ],
    ).toEqual(
      plan().settingsConfig.codexRouting.subagentV2.profiles["offline-writer"],
    );
  });

  it("renders the backend's redacted invalid DTO and repairs from a generic label", async () => {
    ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles[
      "deepseek-v4-flash"
    ] = {
      model: "RAW_MODEL_MUST_NOT_RENDER",
      enabled: true,
    };
    delete ipcState.providers.router.settingsConfig.codexRouting.subagentV2
      .profiles["repository-scout"];
    ipcState.statusResponse = {
      ...statusFixture,
      profiles: [
        {
          routable: false,
          status: "invalid",
          nonGenerationReason: "invalid",
          warnings: [],
        },
        statusFixture.profiles[1],
      ],
    };

    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    expect(
      (await screen.findAllByText("生成状态：invalid")).length,
    ).toBeGreaterThan(0);
    expect(screen.queryByText(/RAW_MODEL_MUST_NOT_RENDER/)).toBeNull();
    const repair = screen.getByRole("button", {
      name: "修复无效能力配置 1",
    });
    await user.click(repair);
    expect(
      within(
        screen.getByRole("region", {
          name: "DeepSeek V4 Flash 子 Agent 能力",
        }),
      ).getByRole("group", { name: "任务优势" }),
    ).toBeVisible();
  });

  it("keeps an invalid secret-bearing profile key out of UI, diagnostics, requests, and persistence", async () => {
    const secretProfileKey = "RAW_PROFILE_KEY_SECRET_SENTINEL";
    const profiles =
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles;
    profiles[secretProfileKey] = {
      model: "deepseek-v4-flash",
      enabled: true,
    };
    delete profiles["repository-scout"];
    ipcState.statusResponse = {
      ...statusFixture,
      profiles: [
        {
          routable: false,
          status: "invalid",
          nonGenerationReason: "invalid",
          warnings: [],
        },
        statusFixture.profiles[1],
      ],
    };

    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    expect(
      await screen.findByRole("region", { name: "无效能力配置 1" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "修复无效能力配置 1" }),
    ).toBeVisible();
    expect(document.body.textContent).not.toContain(secretProfileKey);
    expect(
      Array.from(document.querySelectorAll("*")).flatMap((element) => [
        element.getAttribute("aria-label"),
        element.getAttribute("aria-description"),
        element.getAttribute("aria-describedby"),
      ]),
    ).not.toContain(secretProfileKey);

    await user.click(
      screen.getByRole("button", { name: "保存 V2 子 Agent 能力配置" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("无效能力配置");
    expect(v2PersistenceCalls()).toHaveLength(0);
    expect(
      vi.mocked(invoke).mock.calls.map(([, args]) => JSON.stringify(args)),
    ).not.toContain(secretProfileKey);
  });

  it.each([
    ["string override container", "RAW_OVERRIDE_STRING"],
    ["null override container", null],
    ["array override container", ["RAW_OVERRIDE_ARRAY"]],
    ["non-string roleName", { roleName: { raw: "RAW_ROLE_NAME" } }],
    ["non-string description", { description: ["RAW_DESCRIPTION"] }],
    [
      "non-string developerInstructions",
      { developerInstructions: { raw: "RAW_DEVELOPER_INSTRUCTIONS" } },
    ],
    ["non-array nicknameCandidates", { nicknameCandidates: "RAW_NICKNAME" }],
    [
      "non-string nicknameCandidates entry",
      { nicknameCandidates: [{ raw: "RAW_NICKNAME_ENTRY" }] },
    ],
    [
      "non-string modelReasoningEffort",
      { modelReasoningEffort: { raw: "RAW_REASONING_EFFORT" } },
    ],
    [
      "unsupported modelReasoningEffort string",
      { modelReasoningEffort: "RAW_REASONING_EFFORT" },
    ],
  ])(
    "isolates %s without dereferencing or reflecting invalid raw content",
    async (_caseName, overrides) => {
      const secretPattern = /RAW_[A-Z_]+/;
      const profiles =
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2
          .profiles;
      profiles["deepseek-v4-flash"] = {
        ...profiles["repository-scout"],
        overrides,
      };
      delete profiles["repository-scout"];
      ipcState.statusResponse = {
        ...statusFixture,
        profiles: [
          {
            routable: false,
            status: "invalid",
            nonGenerationReason: "invalid",
            warnings: [],
          },
          statusFixture.profiles[1],
        ],
      };

      const user = userEvent.setup();
      await mountWorkspaceFromPersistedPlan();

      expect(
        await screen.findByRole("button", {
          name: "修复无效能力配置 1",
        }),
      ).toBeVisible();
      expect(document.body.textContent).not.toMatch(secretPattern);
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(
            ([command, args]) =>
              command === "preview_codex_subagent_profile" &&
              (args as Record<string, unknown> | undefined)?.model ===
                "deepseek-v4-flash",
          ),
      ).toHaveLength(0);

      await user.click(
        screen.getByRole("button", {
          name: "修复无效能力配置 1",
        }),
      );
      expect(
        within(flashRegion()).getByRole("group", { name: "任务优势" }),
      ).toBeVisible();
      expect(document.body.textContent).not.toMatch(secretPattern);
    },
  );

  it("renders preview, authoritative status, and TOML inside each profile region", async () => {
    await renderWorkspace();

    const flash = within(flashRegion());
    const pro = within(proRegion());
    expect(
      await flash.findByText(previewFixture.requestedRoleName),
    ).toBeVisible();
    expect(flash.getByText("repository-scout：generated")).toBeVisible();
    expect(
      flash.getByText(previewFixture.tomlPreview, {
        normalizer: getDefaultNormalizer({
          trim: false,
          collapseWhitespace: false,
        }),
      }),
    ).toBeVisible();
    expect(
      (await pro.findAllByText(proPreviewFixture.requestedRoleName)).length,
    ).toBeGreaterThan(0);
    expect(pro.getByText("offline-writer：unroutable")).toBeVisible();
    expect(
      pro.getByText(proPreviewFixture.tomlPreview, {
        normalizer: getDefaultNormalizer({
          trim: false,
          collapseWhitespace: false,
        }),
      }),
    ).toBeVisible();
  });

  it("disables draft controls while a save transaction is pending", async () => {
    const gate = createDeferred();
    ipcState.updateGate = gate.promise;
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();
    const policy = await screen.findByLabelText("第三方子 Agent 选择策略");

    await user.click(
      screen.getByRole("button", { name: "保存 V2 子 Agent 能力配置" }),
    );
    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
    expect(policy).toBeDisabled();
    expect(within(flashRegion()).getByLabelText("角色描述")).toBeDisabled();

    gate.resolve();
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "保存 V2 子 Agent 能力配置",
        }),
      ).toBeEnabled(),
    );
  });

  it("merges a concurrent provider catalog refresh instead of rolling it back", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    ipcState.beforeV2Persistence = () => {
      ipcState.providers.router.settingsConfig.modelCatalog = {
        ...ipcState.providers.router.settingsConfig.modelCatalog,
        refreshedAlias: { "deepseek-v4-flash": "flash-live" },
      };
      ipcState.providers.router.settingsConfig.codexRouting.catalogAliases = {
        flash: "deepseek-v4-flash-live",
      };
    };
    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );

    await saveV2(user);

    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
    expect(updateProviderCalls()).toHaveLength(0);
    expect(latestSavedPlan()?.settingsConfig.modelCatalog).toEqual(
      expect.objectContaining({
        refreshedAlias: { "deepseek-v4-flash": "flash-live" },
      }),
    );
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.selectionPolicy,
    ).toBe("official_first");
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.catalogAliases,
    ).toEqual({ flash: "deepseek-v4-flash-live" });
  });

  it("shows preview absence explicitly without inventing a medium effort", async () => {
    delete ipcState.providers.router.settingsConfig.codexRouting.subagentV2
      .profiles["repository-scout"].overrides.modelReasoningEffort;
    ipcState.previewErrors["deepseek-v4-flash"] = "preview unavailable";

    await mountWorkspaceFromPersistedPlan();

    const region = within(flashRegion());
    expect(await region.findByText("preview unavailable")).toHaveAttribute(
      "role",
      "alert",
    );
    expect(region.getByLabelText("模型推理强度")).toHaveValue("");
  });

  it("surfaces authoritative status IPC failures", async () => {
    ipcState.statusError = "status bridge unavailable";

    await renderWorkspace();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "status bridge unavailable",
    );
  });

  it("uses multiline controls for generated descriptions and instructions", async () => {
    await renderWorkspace();

    expect(within(flashRegion()).getByLabelText("角色描述").tagName).toBe(
      "TEXTAREA",
    );
    expect(within(flashRegion()).getByLabelText("开发者指令").tagName).toBe(
      "TEXTAREA",
    );
  });
});

describe("Codex Sub-Agent V2 new-plan capability defaults", () => {
  it("persists exact schema-v1 defaults through the workspace create action", async () => {
    const user = userEvent.setup();
    await mountWorkspaceWithoutPlan();
    await user.click(
      (await screen.findAllByRole("button", { name: "创建多路路由" }))[0],
    );
    await waitFor(() => expect(addProviderCalls()).toHaveLength(1));

    expect(addProviderCalls()[0]).toEqual([
      "add_provider",
      {
        provider: expect.objectContaining({
          settingsConfig: expect.objectContaining({
            codexRouting: expect.objectContaining({
              subagentV2: {
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
                      preference: "preferred",
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
                      preference: "preferred",
                      reasoningEffort: "high",
                    },
                  },
                },
              },
            }),
          }),
        }),
        app: "codex",
        addToLive: false,
      },
    ]);
  });

  it("persists exact schema-v1 defaults when the wizard publishes a new V2 plan", async () => {
    const source = provider();
    ipcState.providers = { [source.id]: source };
    const wizard = await mountWizardFromPersistedPlan();
    await wizard.user.click(screen.getByRole("button", { name: "保存并发布" }));
    await wizard.user.click(
      screen.getAllByRole("button", { name: "保存并发布" }).at(-1)!,
    );
    await waitFor(() => expect(addProviderCalls()).toHaveLength(1));

    expect(addProviderCalls()[0]).toEqual([
      "add_provider",
      {
        provider: expect.objectContaining({
          settingsConfig: expect.objectContaining({
            codexRouting: expect.objectContaining({
              subagentV2: {
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
                      preference: "preferred",
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
                      preference: "preferred",
                      reasoningEffort: "high",
                    },
                  },
                },
              },
            }),
          }),
        }),
        app: "codex",
        addToLive: false,
      },
    ]);
  });

  it("keeps an existing legacy V2 plan uninitialized through an ordinary wizard save", async () => {
    seedPersistedPlan(false);
    ipcState.providers.router.settingsConfig.codexRouting.subagentVersion =
      "v2";
    const wizard = await mountWizardFromPersistedPlan();
    expect(
      await screen.findByRole("button", {
        name: "初始化 V2 子 Agent 能力配置",
      }),
    ).toBeInTheDocument();
    await wizard.user.click(screen.getByRole("button", { name: "保存并发布" }));
    await wizard.user.click(
      screen.getAllByRole("button", { name: "保存并发布" }).at(-1)!,
    );
    await waitFor(() => expect(updateProviderCalls()).toHaveLength(1));

    expect(updateProviderCalls()[0]).toEqual([
      "update_provider",
      {
        provider: expect.objectContaining({
          id: "router",
        }),
        app: "codex",
        originalId: undefined,
      },
    ]);
    const savedProvider = (
      updateProviderCalls()[0][1] as { provider: Provider }
    ).provider;
    expect(savedProvider.settingsConfig.codexRouting).not.toHaveProperty(
      "subagentV2",
    );
  });
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
    "saves policy option %s as %s through the focused atomic command",
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
      expect(invoke).toHaveBeenCalledWith("update_codex_subagent_v2", {
        providerId: "router",
        subagentV2: expect.objectContaining({
          selectionPolicy: "third_party_first",
          profiles: expect.objectContaining({
            "repository-scout": expect.objectContaining({
              questionnaire: expect.objectContaining({
                optimization: "quality",
              }),
              overrides: expect.objectContaining({
                description: "Persisted manual description",
              }),
            }),
          }),
        }),
      }),
    );
    expect(v2PersistenceCalls()).toHaveLength(1);
    expect(updateProviderCalls()).toHaveLength(0);
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
    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
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

  it("initializes the exact legacy V2 defaults with one focused atomic write", async () => {
    const user = userEvent.setup();
    await renderWorkspace(false);
    await user.click(
      await screen.findByRole("button", {
        name: "初始化 V2 子 Agent 能力配置",
      }),
    );
    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
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
            preference: "preferred",
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
            preference: "preferred",
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
    const output = within(flashBackendRegion());
    expect(await output.findByText("请求角色名")).toBeInTheDocument();
    expect(output.getByText("repository-scout")).toBeInTheDocument();
    expect(output.getByText("实际角色名")).toBeInTheDocument();
    expect(output.getByText("repository-scout-2")).toBeInTheDocument();
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
    expect(
      (await within(flashBackendRegion()).findAllByText(value)).length,
    ).toBeGreaterThan(0);
  });

  it("renders backend-returned backend TOML", async () => {
    await renderWorkspace();
    expect(
      await within(flashBackendRegion()).findByText(
        previewFixture.tomlPreview,
        {
          normalizer: getDefaultNormalizer({
            trim: false,
            collapseWhitespace: false,
          }),
        },
      ),
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
    ["provider kind", "Provider 类型：third_party", "flash"],
    ["routable flag", "可路由：是", "flash"],
    ["enabled flag", "已启用：是", "flash"],
    ["role-name source", "角色名称来源：override", "flash"],
    ["description source", "角色描述来源：automatic", "flash"],
    ["developer-instructions source", "开发者指令来源：override", "flash"],
    ["nickname-candidates source", "昵称候选来源：automatic", "flash"],
    ["model-reasoning source", "模型推理强度来源：override", "flash"],
    [
      "absolute role path",
      "C:\\Codex\\agents\\repository-scout-2.toml",
      "flash",
    ],
    ["model", "模型：deepseek-v4-flash", "flash"],
    ["model provider", "模型 Provider：codex_model_router_v2", "flash"],
    ["reasoning effort", "推理强度：medium", "flash"],
    ["generation source", "生成来源：configured_profiles", "global"],
    ["generated status", "生成状态：generated", "flash"],
    ["second profile status", "offline-writer：unroutable", "pro"],
    ["second profile controlled reason", "未生成原因：unroutable", "pro"],
  ])("renders authoritative %s", async (_field, value, scope) => {
    await renderWorkspace();
    const output =
      scope === "flash"
        ? within(flashBackendRegion())
        : scope === "pro"
          ? within(proBackendRegion())
          : screen;
    expect(await output.findByText(value)).toBeInTheDocument();
  });

  it("renders authoritative requested and effective roles separately", async () => {
    await renderWorkspace();
    const output = within(flashBackendRegion());
    expect(await output.findByText("请求角色名")).toBeInTheDocument();
    expect(output.getByText("repository-scout")).toBeInTheDocument();
    expect(output.getByText("实际角色名")).toBeInTheDocument();
    expect(output.getByText("repository-scout-2")).toBeInTheDocument();
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
