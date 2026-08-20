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
import type {
  CodexSubagentProfilePreview,
  CodexSubagentReasoningCapability,
  CodexSubagentV2Profile,
} from "@/types/codexSubagentV2";
import { CodexMultiRouterWizard } from "./CodexMultiRouterWizard";
import { CodexRouterWorkspacePage } from "./CodexRouterWorkspacePage";

const deepseekReasoningCapability: CodexSubagentReasoningCapability = {
  supportKind: "effort_levels" as const,
  source: "builtin",
  confidence: "confirmed" as const,
  codexSelectableEfforts: ["none", "low", "medium", "high", "xhigh", "max"],
  providerAcceptedEfforts: ["low", "high", "max"],
  providerDefaultEffort: "high" as const,
  disableAllowed: true,
  effortMap: {
    low: "low" as const,
    medium: "high" as const,
    high: "high" as const,
    xhigh: "high" as const,
    max: "max" as const,
  },
};

const unknownReasoningCapability: CodexSubagentReasoningCapability = {
  supportKind: "unknown",
  source: null,
  confidence: "unverified",
  codexSelectableEfforts: [],
  providerAcceptedEfforts: [],
  providerDefaultEffort: null,
  disableAllowed: false,
  effortMap: {},
};

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
  reasoningPolicy: "fixed" as const,
  reasoningCapability: deepseekReasoningCapability,
  modelContextWindow: 128000,
  tomlPreview:
    '[agents.repository-scout-2]\nmodel = "deepseek-v4-flash"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "medium"',
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
  reasoningPolicy: "fixed" as const,
  reasoningCapability: deepseekReasoningCapability,
  modelContextWindow: 256000,
  tomlPreview:
    '[agents.deep-reviewer]\nmodel = "deepseek-v4-pro"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "high"',
  warnings: ["Pro profile warning."],
} satisfies CodexSubagentProfilePreview;

const qwenPreviewFixture = {
  providerKind: "third_party" as const,
  requestedRoleName: "qwen3-6",
  effectiveRoleName: "qwen3-6",
  description: "Catalog draft awaiting explicit enablement.",
  developerInstructions: "Remain disabled until the user reviews this draft.",
  nicknameCandidates: ["Qwen"],
  model: "qwen3.6",
  modelProvider: "codex_model_router_v2" as const,
  modelReasoningEffort: "medium" as const,
  reasoningPolicy: "fixed" as const,
  reasoningCapability: deepseekReasoningCapability,
  modelContextWindow: 262144,
  tomlPreview:
    '[agents.qwen3-6]\nmodel = "qwen3.6"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "medium"',
  warnings: ["Catalog profile remains disabled by default."],
} satisfies CodexSubagentProfilePreview;

const backendDraftPreviewFixture = {
  providerKind: "third_party" as const,
  requestedRoleName: "qwen-draft",
  effectiveRoleName: "qwen-draft",
  description: "Backend-created catalog draft.",
  developerInstructions: "Keep this backend-created profile disabled.",
  nicknameCandidates: ["Qwen Draft"],
  model: "QWEN-ＤＲＡＦＴ",
  modelProvider: "codex_model_router_v2" as const,
  modelReasoningEffort: "medium" as const,
  reasoningPolicy: "fixed" as const,
  reasoningCapability: deepseekReasoningCapability,
  modelContextWindow: 131072,
  tomlPreview:
    '[agents.qwen-draft]\nmodel = "QWEN-ＤＲＡＦＴ"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "medium"',
  warnings: ["Backend-created profile remains disabled by default."],
} satisfies CodexSubagentProfilePreview;

const officialDraftPreviewFixture = {
  providerKind: "official" as const,
  requestedRoleName: "official-integrator",
  effectiveRoleName: "official-integrator",
  description: "Backend-created official catalog draft.",
  developerInstructions: "Keep the official draft disabled until reviewed.",
  nicknameCandidates: ["Integrator"],
  model: "gpt-5.6-sol",
  modelProvider: "codex_model_router_v2" as const,
  modelReasoningEffort: "high" as const,
  reasoningPolicy: "fixed" as const,
  reasoningCapability: deepseekReasoningCapability,
  modelContextWindow: 262144,
  tomlPreview:
    '[agents.official-integrator]\nmodel = "gpt-5.6-sol"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "high"',
  warnings: ["Official catalog profile remains disabled by default."],
} satisfies CodexSubagentProfilePreview;

const previewFixturesByModel: Record<string, CodexSubagentProfilePreview> = {
  "deepseek-v4-flash": previewFixture,
  "deepseek-v4-pro": proPreviewFixture,
  "qwen3.6": qwenPreviewFixture,
  "QWEN-ＤＲＡＦＴ": backendDraftPreviewFixture,
  "gpt-5.6-sol": officialDraftPreviewFixture,
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
  nextInitializedProvider: null as Provider | null,
  nextProjection: null as null | {
    status: "applied" | "not_required" | "pending_retry";
    warning?: { code: string; message: string };
  },
  nextVerification: null as Record<string, unknown> | null,
  omitWriteVerification: false,
}));

// The only test double is the real frontend process boundary. The dispatcher
// keeps the same provider record that get_providers/update_provider use in the
// application, so save/remount tests cannot pass through a second local schema.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    const mutationResult = (savedProvider: Provider) => {
      const projection = ipcState.nextProjection ?? { status: "applied" };
      const roleFilesStatus =
        projection.status === "not_required"
          ? "not_required"
          : projection.status === "pending_retry"
            ? "pending_retry"
            : "verified";
      const verification = ipcState.nextVerification ?? {
        databasePersisted: true,
        roleFilesStatus,
        roleFiles:
          projection.status === "applied"
            ? [
                {
                  profileKey: "repository-scout",
                  path: "C:\\Codex\\agents\\repository-scout-2.toml",
                  exists: true,
                  contentMatches: true,
                },
              ]
            : [],
        activation: "restart_codex_and_start_new_session",
      };
      return {
        ...savedProvider,
        projection,
        ...(ipcState.omitWriteVerification ? {} : { verification }),
      };
    };
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
        return JSON.parse(JSON.stringify(mutationResult(savedProvider)));
      }
      case "initialize_codex_subagent_v2": {
        if (!args || typeof args.providerId !== "string") {
          throw new Error("initialize_codex_subagent_v2 requires providerId");
        }
        const latest = ipcState.providers[args.providerId];
        if (!latest) throw new Error("provider not found");
        const initialized = ipcState.nextInitializedProvider
          ? (JSON.parse(
              JSON.stringify(ipcState.nextInitializedProvider),
            ) as Provider)
          : (JSON.parse(JSON.stringify(latest)) as Provider);
        initialized.id = args.providerId;
        if (!ipcState.nextInitializedProvider) {
          initialized.settingsConfig.codexRouting.subagentV2 = {
            schemaVersion: 2,
            selectionPolicy: "balanced",
            profiles: {
              "qwen-draft": {
                model: "QWEN-ＤＲＡＦＴ",
                enabled: false,
                questionnaire: {
                  taskStrengths: ["repository_exploration"],
                  optimization: "balanced",
                  writeScope: "read_only",
                  preference: "eligible",
                },
                reasoning: { policy: "delegated" },
              },
            },
          };
        }
        ipcState.providers[initialized.id] = initialized;
        return JSON.parse(JSON.stringify(mutationResult(initialized)));
      }
      case "reconcile_codex_subagent_v2_profiles": {
        if (
          !args ||
          typeof args.providerId !== "string" ||
          ![
            "sync_catalog",
            "remove_all_invalid",
            "recover_all_invalid_from_catalog",
            "prune_unroutable",
          ].includes(String(args.action))
        ) {
          throw new Error(
            "reconcile_codex_subagent_v2_profiles requires a controlled backend action",
          );
        }
        if (typeof args.subagentV2 !== "object" || args.subagentV2 === null) {
          throw new Error(
            "every reconcile action requires the complete current unsaved subagentV2 draft",
          );
        }
        const latest = ipcState.providers[args.providerId];
        if (!latest) throw new Error("provider not found");
        const savedProvider = JSON.parse(JSON.stringify(latest)) as Provider;
        savedProvider.settingsConfig.codexRouting.subagentV2 = JSON.parse(
          JSON.stringify(args.subagentV2),
        );
        const config = savedProvider.settingsConfig.codexRouting.subagentV2;
        if (args.action === "sync_catalog") {
          config.profiles["qwen3.6"] = {
            model: "qwen3.6",
            enabled: false,
            questionnaire: {
              taskStrengths: ["repository_exploration"],
              optimization: "balanced",
              writeScope: "read_only",
              preference: "eligible",
            },
            reasoning: { policy: "delegated" },
          };
        } else if (args.action === "remove_all_invalid") {
          delete config.profiles.RAW_INVALID_PROFILE_KEY_ALPHA;
          delete config.profiles.RAW_INVALID_PROFILE_KEY_BETA;
        } else if (args.action === "recover_all_invalid_from_catalog") {
          const aliasProfile = config.profiles.LEGACY_ALIAS_SENTINEL;
          delete config.profiles.LEGACY_ALIAS_SENTINEL;
          delete config.profiles.RAW_INVALID_PROFILE_KEY_ALPHA;
          delete config.profiles.RAW_INVALID_PROFILE_KEY_BETA;
          if (aliasProfile) {
            config.profiles["deepseek-v4-flash"] = aliasProfile;
          }
          config.profiles["deepseek-v4-pro"] = {
            model: "deepseek-v4-pro",
            enabled: false,
            questionnaire: {
              taskStrengths: ["complex_implementation"],
              optimization: "quality",
              writeScope: "complex_changes",
              preference: "eligible",
            },
            reasoning: { policy: "delegated" },
          };
          config.profiles["qwen3.6"] = {
            model: "qwen3.6",
            enabled: false,
            questionnaire: {
              taskStrengths: ["repository_exploration"],
              optimization: "balanced",
              writeScope: "read_only",
              preference: "eligible",
            },
            reasoning: { policy: "delegated" },
          };
        } else if (args.action === "prune_unroutable") {
          // The explicit "sync with catalog" action removes every unroutable
          // profile (model left the routable catalog), regardless of enabled state.
          delete config.profiles["offline-writer"];
        }
        ipcState.providers[savedProvider.id] = savedProvider;
        return JSON.parse(JSON.stringify(mutationResult(savedProvider)));
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
        const reasoning = (args?.profile as CodexSubagentV2Profile | undefined)
          ?.reasoning;
        const effort =
          reasoning?.policy === "fixed" ? reasoning.effort : undefined;
        const effectiveEffort =
          reasoning?.policy === "disabled"
            ? "none"
            : reasoning?.policy === "model_default"
              ? "high"
              : effort;
        const baseToml = fixture.tomlPreview.replace(
          /\nmodel_reasoning_effort = "[^"]+"$/,
          "",
        );
        const response = {
          ...fixture,
          reasoningPolicy: reasoning?.policy ?? "delegated",
          reasoningCapability: deepseekReasoningCapability,
          ...(effectiveEffort
            ? { modelReasoningEffort: effectiveEffort }
            : { modelReasoningEffort: undefined }),
          tomlPreview: effectiveEffort
            ? `${baseToml}\nmodel_reasoning_effort = "${effectiveEffort}"`
            : baseToml,
        };
        return JSON.parse(JSON.stringify(response));
      }
      case "get_codex_subagent_reasoning_capabilities":
        return {
          "deepseek-v4-flash": deepseekReasoningCapability,
          "deepseek-v4-pro": deepseekReasoningCapability,
        };
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

function currentV2Route() {
  return {
    id: "flash-route",
    enabled: true,
    targetProviderId: "third-party",
    modelSelection: { mode: "all" as const },
    matchPrefixes: [] as string[],
    aliases: {},
    authPolicy: { source: "provider_config" as const },
  };
}

function plan(withV2 = true): Provider {
  return {
    id: "router",
    name: "Codex MultiRouter",
    category: "custom",
    settingsConfig: {
      codexRouting: {
        schemaVersion: 2,
        enabled: true,
        routes: [currentV2Route()],
        ...(withV2
          ? {
              subagentV2: {
                schemaVersion: 2,
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
                    },
                    reasoning: { policy: "fixed", effort: "medium" },
                    overrides: {
                      roleName: "repository-scout",
                      developerInstructions: "Do not modify source files.",
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
                    },
                    reasoning: { policy: "fixed", effort: "high" },
                  },
                },
              },
            }
          : {}),
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

function officialProvider(): Provider {
  return {
    id: "codex-official",
    name: "OpenAI Official",
    category: "official",
    settingsConfig: {
      auth: {},
      modelCatalog: {
        models: [{ model: "gpt-5.6-sol", contextWindow: 262144 }],
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

function preservedValidProfile(): CodexSubagentV2Profile {
  return {
    model: "deepseek-v4-flash",
    enabled: true,
    questionnaire: {
      taskStrengths: ["repository_exploration", "testing"],
      optimization: "quality",
      writeScope: "bounded_changes",
      preference: "fallback",
    },
    reasoning: { policy: "fixed", effort: "xhigh" },
    overrides: {
      roleName: "keep-valid-role",
      description: "KEEP_VALID_DESCRIPTION",
      developerInstructions: "KEEP_VALID_INSTRUCTIONS",
      nicknameCandidates: ["KeepValid", "Stable"],
    },
  };
}

function seedMalformedProfiles() {
  const catalog =
    ipcState.providers["third-party"].settingsConfig.modelCatalog.models;
  catalog.push({
    model: "qwen3.6",
    displayName: "Qwen 3.6",
    contextWindow: 262144,
  });
  const profiles =
    ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles;
  profiles["repository-scout"] = preservedValidProfile();
  profiles.RAW_INVALID_PROFILE_KEY_ALPHA = {
    model: "deepseek-v4-pro",
    enabled: true,
  };
  profiles.RAW_INVALID_PROFILE_KEY_BETA = {
    model: "qwen3.6",
    enabled: true,
  };
  delete profiles["offline-writer"];
  ipcState.statusResponse = {
    ...statusFixture,
    profiles: [
      {
        routable: false,
        status: "invalid",
        nonGenerationReason: "invalid",
        warnings: [],
      },
      {
        routable: false,
        status: "invalid",
        nonGenerationReason: "invalid",
        warnings: [],
      },
    ],
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
        initialTab="subagents"
        isProxyRunning={false}
        isCodexTakeoverActive={false}
        onEditProvider={vi.fn()}
        onDeletePlan={vi.fn()}
        onCreateProvider={vi.fn()}
      />
    </QueryClientProvider>,
  );
  await screen.findByRole("tab", { name: "子 Agent" });
  if (selectedPlan.settingsConfig.codexRouting.subagentV2) {
    await waitFor(() =>
      expect(
        vi
          .mocked(invoke)
          .mock.calls.some(
            ([command]) => command === "get_codex_subagent_profile_statuses",
          ),
      ).toBe(true),
    );
  } else {
    await screen.findByRole("button", {
      name: "初始化 V2 子 Agent 能力配置",
    });
  }
  return { ...result, selectedPlan };
}

async function renderWorkspace(withV2 = true) {
  seedPersistedPlan(withV2);
  return mountWorkspaceFromPersistedPlan();
}

async function mountWorkspaceWithoutPlan(source = provider()) {
  ipcState.providers = { [source.id]: source };
  const loaded = await providersApi.getAll("codex");
  const queryClient = createQueryClient();
  queryClient.setQueryData(["providers", "codex"], {
    currentProviderId: source.id,
    providers: loaded,
  });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <CodexRouterWorkspacePage
        providers={Object.values(loaded)}
        activeProviderId={source.id}
        initialProviderId={source.id}
        initialTab="subagents"
        isProxyRunning={false}
        isCodexTakeoverActive={false}
        onEditProvider={vi.fn()}
        onDeletePlan={vi.fn()}
        onCreateProvider={vi.fn()}
      />
    </QueryClientProvider>,
  );
  await screen.findByRole("tab", { name: "子 Agent" });
  await screen.findByText(
    "先创建或选择一个多路路由方案，再配置它的子 Agent 协议和模型能力。",
  );
  return { ...result, source, queryClient };
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
  await user.click(await screen.findByRole("button", { name: "启用并验证" }));
  return { ...result, user };
}

async function renderPersistedWorkspace() {
  seedPersistedPlan(true);
  const result = await mountWorkspaceFromPersistedPlan();
  return { ...result, user: userEvent.setup() };
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
      name: "保存 V2 子 Agent 配置",
    }),
  );
}

type FocusedV2Mutation = "initialize" | "reconcile";

async function runFocusedV2Mutation(operation: FocusedV2Mutation) {
  const user = userEvent.setup();
  await renderWorkspace(operation !== "initialize");
  await user.click(
    await screen.findByRole("button", {
      name:
        operation === "initialize"
          ? "初始化 V2 子 Agent 能力配置"
          : "添加第三方可配置模型",
    }),
  );
}

function appliedVerification(overrides: Record<string, unknown> = {}) {
  return {
    databasePersisted: true,
    roleFilesStatus: "verified",
    roleFiles: [
      {
        profileKey: "repository-scout",
        path: "C:\\Codex\\agents\\repository-scout-2.toml",
        exists: true,
        contentMatches: true,
      },
    ],
    activation: "restart_codex_and_start_new_session",
    ...overrides,
  };
}

function focusedMutationSuccessPattern(operation: FocusedV2Mutation) {
  return operation === "initialize"
    ? /V2 子 Agent 能力配置已初始化/
    : /已添加第三方可配置模型/;
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

async function openProfile(user: UserEvent, model: string) {
  const trigger = await screen.findByRole("button", {
    name: new RegExp(`配置 ${model}`, "i"),
  });
  if (trigger.getAttribute("aria-expanded") !== "true") {
    await user.click(trigger);
  }
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  return screen.getByRole("region", { name: `${model} 子 Agent 配置` });
}

function flashRegion() {
  return screen.getByRole("region", {
    name: "deepseek-v4-flash 子 Agent 配置",
  });
}

async function expectPreservedValidProfileInUi(user: UserEvent) {
  const region = await openAdvancedFields(user);
  expect(
    screen.getByRole("switch", {
      name: "启用 deepseek-v4-flash 作为 V2 子 Agent",
    }),
  ).toBeChecked();
  expect(within(region).getByLabelText("优化目标")).toHaveValue("quality");
  expect(within(region).getByLabelText("写入范围")).toHaveValue(
    "bounded_changes",
  );
  expect(within(region).getByLabelText("模型偏好")).toHaveValue("fallback");
  expect(within(region).getByLabelText("推理策略")).toHaveValue("fixed");
  expect(within(region).getByLabelText("角色名称")).toHaveValue(
    "keep-valid-role",
  );
  expect(within(region).getByLabelText("角色描述")).toHaveValue(
    "KEEP_VALID_DESCRIPTION",
  );
  expect(within(region).getByLabelText("开发者指令")).toHaveValue(
    "KEEP_VALID_INSTRUCTIONS",
  );
  expect(within(region).getByLabelText("昵称候选")).toHaveValue(
    "KeepValid, Stable",
  );
  expect(within(region).getByLabelText("模型推理强度")).toHaveValue("xhigh");
}

async function openAdvancedFields(
  user: UserEvent,
  model = "deepseek-v4-flash",
) {
  const region = await openProfile(user, model);
  const trigger = within(region).getByRole("button", { name: "高级字段" });
  if (trigger.getAttribute("aria-expanded") !== "true") {
    await user.click(trigger);
  }
  return region;
}

async function openGeneratedOutput(
  user: UserEvent,
  model = "deepseek-v4-flash",
) {
  const region = await openProfile(user, model);
  const trigger = within(region).getByRole("button", {
    name: "生成结果与 TOML",
  });
  if (trigger.getAttribute("aria-expanded") !== "true") {
    await user.click(trigger);
  }
  return region;
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
  ipcState.nextInitializedProvider = null;
  ipcState.nextProjection = null;
  ipcState.nextVerification = null;
  ipcState.omitWriteVerification = false;
  vi.mocked(invoke).mockClear();
});

describe.each([
  ["initialize", "初始化"] as const,
  ["reconcile", "目录同步"] as const,
])("Codex Sub-Agent V2 %s mutation verification", (operation, label) => {
  it(`${label}在后端省略 verification 时不得成功`, async () => {
    ipcState.omitWriteVerification = true;

    await runFocusedV2Mutation(operation);

    expect(
      await screen.findByText("后端未返回 Codex Agent 文件写入验证结果。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(focusedMutationSuccessPattern(operation)),
    ).toBeNull();
  });

  it(`${label}在 roleFilesStatus=failed 时不得成功`, async () => {
    ipcState.nextVerification = appliedVerification({
      roleFilesStatus: "failed",
      roleFiles: [],
    });

    await runFocusedV2Mutation(operation);

    expect(
      await screen.findByText("Codex Agent 文件写入后回读校验失败。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(focusedMutationSuccessPattern(operation)),
    ).toBeNull();
  });

  it(`${label}在 Agent 文件内容不匹配时不得成功`, async () => {
    ipcState.nextVerification = appliedVerification({
      roleFiles: [
        {
          profileKey: "repository-scout",
          path: "C:\\Codex\\agents\\repository-scout-2.toml",
          exists: true,
          contentMatches: false,
        },
      ],
    });

    await runFocusedV2Mutation(operation);

    expect(
      await screen.findByText("Codex Agent 文件写入后回读校验失败。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(focusedMutationSuccessPattern(operation)),
    ).toBeNull();
  });

  it(`${label}在 activation 不匹配时不得成功`, async () => {
    ipcState.nextVerification = appliedVerification({
      activation: "already_active",
    });

    await runFocusedV2Mutation(operation);

    expect(
      await screen.findByText("后端返回的 Codex Agent 激活条件不一致。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(focusedMutationSuccessPattern(operation)),
    ).toBeNull();
  });
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
    await waitFor(() => expect(updateProviderCalls()).toHaveLength(1));

    expect(latestSavedPlan()?.settingsConfig.codexRouting).toEqual(
      expect.objectContaining({
        futureRouting: { mode: "keep-me" },
        subagentV2: originalV2,
      }),
    );
  });

  it("keeps V2 preview and status requests out of the four-step routing wizard", async () => {
    await mountWizardFromPersistedPlan();

    expect(
      screen.queryByRole("button", { name: /Sub-Agent V2/ }),
    ).not.toBeInTheDocument();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(
          ([command]) => command === "get_codex_subagent_profile_statuses",
        ),
    ).toHaveLength(0);
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
      name: "保存 V2 子 Agent 配置",
    });
    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );

    await user.click(saveButton);
    await waitFor(() => expect(saveButton).toBeEnabled());

    expect(updateProviderCalls()).toHaveLength(0);
    expect(screen.getByRole("alert")).toHaveTextContent(/collision/i);
  });

  it("blocks persistence when an enabled routable profile has unknown reasoning capability", async () => {
    ipcState.statusResponse = {
      ...statusFixture,
      profiles: [
        {
          ...statusFixture.profiles[0],
          reasoningCapability: unknownReasoningCapability,
        },
        statusFixture.profiles[1],
      ],
    };
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );

    await user.click(
      screen.getByRole("button", { name: "保存 V2 子 Agent 配置" }),
    );

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "repository-scout：推理能力未配置，请先在模型目录中声明能力后再保存。",
      ),
    );
    expect(v2PersistenceCalls()).toHaveLength(0);
    expect(updateProviderCalls()).toHaveLength(0);
  });

  it("allows saving while an enabled retained profile is authoritatively unroutable", async () => {
    ipcState.previewErrors["deepseek-v4-pro"] =
      "No enabled route resolves this model.";
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();
    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );

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
    const repaired = await openProfile(user, "deepseek-v4-flash");
    expect(
      within(repaired).getByRole("group", { name: "任务优势" }),
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
    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );

    await user.click(
      screen.getByRole("button", { name: "保存 V2 子 Agent 配置" }),
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
      "non-string fixed reasoning effort",
      undefined,
      { policy: "fixed", effort: { raw: "RAW_REASONING_EFFORT" } },
    ],
    [
      "unsupported fixed reasoning effort string",
      undefined,
      { policy: "fixed", effort: "RAW_REASONING_EFFORT" },
    ],
  ] as Array<[string, unknown, unknown?]>)(
    "isolates %s without dereferencing or reflecting invalid raw content",
    async (_caseName, overrides, reasoning) => {
      const secretPattern = /RAW_[A-Z_]+/;
      const profiles =
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2
          .profiles;
      profiles["deepseek-v4-flash"] = {
        ...profiles["repository-scout"],
        ...(overrides === undefined ? {} : { overrides }),
        ...(reasoning === undefined ? {} : { reasoning }),
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
      await openProfile(user, "deepseek-v4-flash");
      expect(
        within(flashRegion()).getByRole("group", { name: "任务优势" }),
      ).toBeVisible();
      expect(document.body.textContent).not.toMatch(secretPattern);
    },
  );

  it("renders preview, authoritative status, and TOML inside each profile region", async () => {
    const user = userEvent.setup();
    await renderWorkspace();

    const flash = within(await openGeneratedOutput(user));
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
    const pro = within(await openGeneratedOutput(user, "deepseek-v4-pro"));
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

  it("renders input modality provenance and conflict in the profile status", async () => {
    ipcState.statusResponse = {
      ...statusFixture,
      profiles: [
        {
          ...statusFixture.profiles[0],
          inputModality: {
            modalities: ["text"],
            source: "route",
            declarations: [
              { source: "profile_explicit", adopted: false },
              { source: "route", declared: ["text"], adopted: true },
              {
                source: "catalog",
                declared: ["text", "image"],
                adopted: false,
              },
              { source: "name_registry", adopted: false },
            ],
            conflict:
              "输入能力声明冲突：route 声明纯文本，模型目录 声明文本+图像",
          },
        },
        statusFixture.profiles[1],
      ],
    };
    const user = userEvent.setup();
    await renderWorkspace();
    const flash = within(await openGeneratedOutput(user));
    expect(await flash.findByText(/输入能力：纯文本/)).toBeVisible();
    expect(flash.getByText(/输入能力声明冲突/)).toBeVisible();
  });

  it("disables draft controls while a save transaction is pending", async () => {
    const gate = createDeferred();
    ipcState.updateGate = gate.promise;
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();
    const policy = await screen.findByLabelText("第三方子 Agent 选择策略");
    const flash = within(await openAdvancedFields(user));
    await chooseOption(user, policy, "官方优先");

    await user.click(
      screen.getByRole("button", { name: "保存 V2 子 Agent 配置" }),
    );
    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
    expect(policy).toBeDisabled();
    expect(flash.getByLabelText("角色描述")).toBeDisabled();

    gate.resolve();
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "保存 V2 子 Agent 配置",
        }),
      ).toBeDisabled(),
    );
  });

  it("merges a concurrent plan metadata refresh instead of rolling it back", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    ipcState.beforeV2Persistence = () => {
      ipcState.providers.router.settingsConfig.concurrentMetadata = {
        source: "live-refresh",
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
    expect(latestSavedPlan()?.settingsConfig.concurrentMetadata).toEqual({
      source: "live-refresh",
    });
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.selectionPolicy,
    ).toBe("official_first");
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.catalogAliases,
    ).toEqual({ flash: "deepseek-v4-flash-live" });
  });

  it("shows preview absence explicitly without inventing a medium effort", async () => {
    ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles[
      "repository-scout"
    ].reasoning = { policy: "delegated" };
    ipcState.previewErrors["deepseek-v4-flash"] = "preview unavailable";

    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    const region = within(await openAdvancedFields(user));
    await openGeneratedOutput(user);
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
    const user = userEvent.setup();
    await renderWorkspace();
    await openAdvancedFields(user);

    expect(within(flashRegion()).getByLabelText("角色描述").tagName).toBe(
      "TEXTAREA",
    );
    expect(within(flashRegion()).getByLabelText("开发者指令").tagName).toBe(
      "TEXTAREA",
    );
  });
});

describe("Codex Sub-Agent V2 new-plan capability defaults", () => {
  it("states that Workspace V2 selection is best-effort guidance and retains built-in roles", async () => {
    await renderWorkspace();

    expect(screen.getAllByText(/best-effort/).length).toBeGreaterThan(0);
    expect(screen.getByText(/问卷与角色说明只提供选择指导/)).toBeVisible();
    expect(screen.getByText(/不保证选择 Flash 或 Pro/)).toBeVisible();
    expect(screen.getByText(/default、worker、explorer/)).toBeVisible();
  });

  it("creates an official-only workspace plan without phantom profiles and adopts backend initialization", async () => {
    ipcState.nextInitializedProvider = {
      id: "backend-will-adopt-persisted-id",
      name: "Backend Initialized Official Plan",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          enabled: true,
          subagentVersion: "v2",
          routes: [],
          subagentV2: {
            schemaVersion: 1,
            selectionPolicy: "balanced",
            profiles: {
              "gpt-5.6-sol": {
                model: "gpt-5.6-sol",
                enabled: false,
                questionnaire: {
                  taskStrengths: ["high_risk_review"],
                  optimization: "quality",
                  writeScope: "complex_changes",
                  preference: "eligible",
                  reasoningEffort: "high",
                },
              },
            },
          },
        },
        modelCatalog: {
          models: [{ model: "gpt-5.6-sol", contextWindow: 262144 }],
        },
      },
    };
    const user = userEvent.setup();
    const { queryClient } = await mountWorkspaceWithoutPlan(officialProvider());
    await user.click(
      (await screen.findAllByRole("button", { name: "创建多路路由" }))[0],
    );
    await waitFor(() => expect(addProviderCalls()).toHaveLength(1));

    const persisted = (addProviderCalls()[0][1] as { provider: Provider })
      .provider;
    expect(persisted.settingsConfig.codexRouting.subagentV2).toBeUndefined();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("initialize_codex_subagent_v2", {
        providerId: persisted.id,
      }),
    );
    await user.click(screen.getByRole("tab", { name: "子 Agent" }));
    await user.click(screen.getByRole("button", { name: /官方模型（高级）/ }));
    expect(await openProfile(user, "gpt-5.6-sol")).toBeInTheDocument();
    expect(
      screen.queryByRole("region", {
        name: "deepseek-v4-flash 子 Agent 配置",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("region", {
        name: "deepseek-v4-pro 子 Agent 配置",
      }),
    ).not.toBeInTheDocument();
    const cached = queryClient.getQueryData<{
      providers: Record<string, Provider>;
    }>(["providers", "codex"]);
    expect(cached?.providers[persisted.id]).toMatchObject({
      id: persisted.id,
      name: "Backend Initialized Official Plan",
      settingsConfig: {
        codexRouting: {
          subagentV2: {
            profiles: {
              "gpt-5.6-sol": expect.objectContaining({
                model: "gpt-5.6-sol",
              }),
            },
          },
        },
      },
    });
  });

  it("publishes a new wizard plan before asking the backend to initialize V2", async () => {
    const source = provider();
    ipcState.providers = { [source.id]: source };
    const wizard = await mountWizardFromPersistedPlan();
    await wizard.user.click(screen.getByRole("button", { name: "保存并发布" }));
    await waitFor(() => expect(addProviderCalls()).toHaveLength(1));

    const persisted = (addProviderCalls()[0][1] as { provider: Provider })
      .provider;
    expect(persisted.settingsConfig.codexRouting.subagentV2).toBeUndefined();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("initialize_codex_subagent_v2", {
        providerId: persisted.id,
      }),
    );
    expect(
      ipcState.providers[persisted.id].settingsConfig.codexRouting.subagentV2
        .profiles,
    ).toEqual({
      "qwen-draft": expect.objectContaining({ model: "QWEN-ＤＲＡＦＴ" }),
    });
  });

  it("keeps an existing uninitialized schema v2 plan uninitialized through an ordinary wizard save", async () => {
    seedPersistedPlan(false);
    ipcState.providers.router.settingsConfig.codexRouting.subagentVersion =
      "v2";
    const wizard = await mountWizardFromPersistedPlan();
    expect(
      screen.queryByRole("button", { name: /Sub-Agent V2/ }),
    ).not.toBeInTheDocument();
    await wizard.user.click(screen.getByRole("button", { name: "保存并发布" }));
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
    expect(
      savedProvider.settingsConfig.codexRouting.subagentV2,
    ).toBeUndefined();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(
          ([command]) => command === "initialize_codex_subagent_v2",
        ),
    ).toHaveLength(0);
  });
});

describe("Codex Sub-Agent V2 backend-owned catalog reconciliation", () => {
  it("keeps official profiles in an explicit advanced section instead of the third-party list", async () => {
    const current =
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2;
    current.profiles["gpt-5.6-sol"] = {
      model: "gpt-5.6-sol",
      enabled: false,
      questionnaire: {
        taskStrengths: ["high_risk_review"],
        optimization: "quality",
        writeScope: "complex_changes",
        preference: "eligible",
      },
      reasoning: { policy: "fixed", effort: "high" },
    };
    ipcState.statusResponse = {
      ...statusFixture,
      profiles: [
        ...statusFixture.profiles,
        {
          profileKey: "gpt-5.6-sol",
          model: "gpt-5.6-sol",
          providerKind: "official",
          enabled: false,
          routable: true,
          status: "disabled",
          warnings: [],
        },
      ],
    };
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    expect(
      screen.queryByRole("button", { name: "配置 gpt-5.6-sol" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /官方模型（高级）/ }));
    expect(
      screen.getByRole("button", { name: "配置 gpt-5.6-sol" }),
    ).toBeVisible();
    expect(screen.getByText(/官方模型通常不需要创建固定角色/)).toBeVisible();
  });

  it("initializes through the backend instead of constructing canonical keys in the frontend", async () => {
    const user = userEvent.setup();
    await renderWorkspace(false);

    await user.click(
      await screen.findByRole("button", {
        name: "初始化 V2 子 Agent 能力配置",
      }),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("initialize_codex_subagent_v2", {
        providerId: "router",
      }),
    );
    const backendDraft = await openProfile(user, "QWEN-ＤＲＡＦＴ");
    expect(
      screen.getByRole("switch", {
        name: "启用 QWEN-ＤＲＡＦＴ 作为 V2 子 Agent",
      }),
    ).not.toBeChecked();
    expect(backendDraft).toBeVisible();
    expect(
      screen.queryByRole("region", {
        name: "deepseek-v4-flash 子 Agent 配置",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("region", {
        name: "deepseek-v4-pro 子 Agent 配置",
      }),
    ).not.toBeInTheDocument();
    expect(v2PersistenceCalls()).toHaveLength(0);
  });

  it("synchronizes every catalog model as a backend-keyed disabled draft", async () => {
    const models =
      ipcState.providers["third-party"].settingsConfig.modelCatalog.models;
    models.push({
      model: "qwen3.6",
      displayName: "Qwen 3.6",
      contextWindow: 262144,
    });
    const currentDraft = JSON.parse(
      JSON.stringify(
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2,
      ),
    );
    const unsavedDescription = "Unsaved draft survives catalog sync.";
    const expectedUnsavedDraft = JSON.parse(JSON.stringify(currentDraft));
    expectedUnsavedDraft.profiles["repository-scout"].overrides.description =
      unsavedDescription;
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    const flashRegion = await openAdvancedFields(user);
    const description = within(flashRegion).getByLabelText("角色描述");
    await user.clear(description);
    await user.type(description, unsavedDescription);
    expect(description).toHaveValue(unsavedDescription);

    await user.click(
      await screen.findByRole("button", {
        name: "添加第三方可配置模型",
      }),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reconcile_codex_subagent_v2_profiles",
        {
          providerId: "router",
          action: "sync_catalog",
          subagentV2: expectedUnsavedDraft,
        },
      ),
    );
    const flashAfterSync = await openAdvancedFields(user);
    expect(within(flashAfterSync).getByLabelText("角色描述")).toHaveValue(
      unsavedDescription,
    );
    expect(
      screen.getByRole("switch", {
        name: "启用 qwen3.6 作为 V2 子 Agent",
      }),
    ).not.toBeChecked();
  });

  it("offers one backend-owned batch removal action for every malformed profile", async () => {
    seedMalformedProfiles();
    const expectedValidProfile = {
      ...preservedValidProfile(),
      questionnaire: {
        ...preservedValidProfile().questionnaire,
        optimization: "speed" as const,
      },
      overrides: {
        ...preservedValidProfile().overrides,
        description: "UNSAVED_REMOVE_DESCRIPTION",
      },
    };
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    expect(
      await screen.findByRole("region", {
        name: "无效能力配置 1",
      }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("region", {
        name: "无效能力配置 2",
      }),
    ).toBeInTheDocument();
    await expectPreservedValidProfileInUi(user);
    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "第三方优先",
    );
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("优化目标"),
      "速度",
    );
    const description = within(flashRegion()).getByLabelText("角色描述");
    await user.clear(description);
    await user.type(description, "UNSAVED_REMOVE_DESCRIPTION");
    const expectedDraft = JSON.parse(
      JSON.stringify(
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2,
      ),
    );
    expectedDraft.selectionPolicy = "third_party_first";
    expectedDraft.profiles["repository-scout"] = expectedValidProfile;
    await user.click(
      screen.getByRole("button", {
        name: "删除全部无效能力配置（2 项）",
      }),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reconcile_codex_subagent_v2_profiles",
        {
          providerId: "router",
          action: "remove_all_invalid",
          subagentV2: expectedDraft,
        },
      ),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("region", {
          name: "无效能力配置 1",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("region", {
        name: "无效能力配置 2",
      }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("第三方子 Agent 选择策略")).toHaveValue(
      "third_party_first",
    );
    expect(within(flashRegion()).getByLabelText("优化目标")).toHaveValue(
      "speed",
    );
    expect(within(flashRegion()).getByLabelText("角色描述")).toHaveValue(
      "UNSAVED_REMOVE_DESCRIPTION",
    );
    expect(within(flashRegion()).getByLabelText("开发者指令")).toHaveValue(
      "KEEP_VALID_INSTRUCTIONS",
    );
    const profilesAfterRemoval =
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles;
    expect(profilesAfterRemoval).toEqual({
      "repository-scout": expectedValidProfile,
    });
    expect(document.body.textContent).not.toContain(
      "RAW_INVALID_PROFILE_KEY_ALPHA",
    );
    expect(document.body.textContent).not.toContain(
      "RAW_INVALID_PROFILE_KEY_BETA",
    );
  });

  it("offers one backend-owned catalog recovery action for every malformed profile", async () => {
    seedMalformedProfiles();
    const expectedValidProfile = {
      ...preservedValidProfile(),
      overrides: {
        ...preservedValidProfile().overrides,
        roleName: "unsaved-recovery-role",
      },
    };
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    expect(
      await screen.findByRole("region", {
        name: "无效能力配置 1",
      }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("region", {
        name: "无效能力配置 2",
      }),
    ).toBeInTheDocument();
    await expectPreservedValidProfileInUi(user);
    const roleName = within(flashRegion()).getByLabelText("角色名称");
    await user.clear(roleName);
    await user.type(roleName, "unsaved-recovery-role");
    const expectedDraft = JSON.parse(
      JSON.stringify(
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2,
      ),
    );
    expectedDraft.profiles["repository-scout"] = expectedValidProfile;
    await user.click(
      screen.getByRole("button", {
        name: "从模型目录恢复全部无效能力配置（2 项）",
      }),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reconcile_codex_subagent_v2_profiles",
        {
          providerId: "router",
          action: "recover_all_invalid_from_catalog",
          subagentV2: expectedDraft,
        },
      ),
    );
    expect(
      await screen.findByRole("region", {
        name: "deepseek-v4-flash 子 Agent 配置",
      }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", {
        name: "配置 deepseek-v4-pro",
      }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", {
        name: "配置 qwen3.6",
      }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("region", {
        name: "无效能力配置 1",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("region", {
        name: "无效能力配置 2",
      }),
    ).not.toBeInTheDocument();
    expect(within(flashRegion()).getByLabelText("角色名称")).toHaveValue(
      "unsaved-recovery-role",
    );
    expect(within(flashRegion()).getByLabelText("角色描述")).toHaveValue(
      "KEEP_VALID_DESCRIPTION",
    );
    const profilesAfterRecovery =
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles;
    expect(Object.keys(profilesAfterRecovery).sort()).toEqual([
      "deepseek-v4-pro",
      "qwen3.6",
      "repository-scout",
    ]);
    expect(profilesAfterRecovery["repository-scout"]).toEqual(
      expectedValidProfile,
    );
    expect(profilesAfterRecovery["deepseek-v4-pro"]).toEqual({
      model: "deepseek-v4-pro",
      enabled: false,
      questionnaire: {
        taskStrengths: ["complex_implementation"],
        optimization: "quality",
        writeScope: "complex_changes",
        preference: "eligible",
      },
      reasoning: { policy: "delegated" },
    });
    expect(profilesAfterRecovery["qwen3.6"]).toEqual({
      model: "qwen3.6",
      enabled: false,
      questionnaire: {
        taskStrengths: ["repository_exploration"],
        optimization: "balanced",
        writeScope: "read_only",
        preference: "eligible",
      },
      reasoning: { policy: "delegated" },
    });
    expect(document.body.textContent).not.toContain(
      "RAW_INVALID_PROFILE_KEY_ALPHA",
    );
    expect(document.body.textContent).not.toContain(
      "RAW_INVALID_PROFILE_KEY_BETA",
    );
  });

  it("offers a backend-owned sync action that removes every unroutable profile", async () => {
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    // The status fixture marks "offline-writer" (deepseek-v4-pro, no route) as
    // unroutable, so the sync button is offered with a count of 1.
    const syncButton = await screen.findByRole("button", {
      name: "与目录同步：删除已失效模型（1 项）",
    });
    expect(syncButton).toBeInTheDocument();

    await user.click(syncButton);

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reconcile_codex_subagent_v2_profiles",
        expect.objectContaining({
          providerId: "router",
          action: "prune_unroutable",
        }),
      ),
    );

    // The unroutable profile is removed from the persisted draft; the routable
    // profile is kept.
    const profilesAfterPrune =
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles;
    expect(Object.keys(profilesAfterPrune)).toEqual(["repository-scout"]);
    expect(profilesAfterPrune["offline-writer"]).toBeUndefined();
  });

  it("offers backend re-key recovery for a structurally usable canonical key mismatch", async () => {
    const legacyProfile = preservedValidProfile();
    const config =
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2;
    config.profiles = {
      LEGACY_ALIAS_SENTINEL: legacyProfile,
    };
    ipcState.statusResponse = {
      mode: "v2",
      generationSource: "configured_profiles",
      profiles: [
        {
          routable: false,
          status: "invalid",
          nonGenerationReason: "invalid",
          warnings: [],
        },
      ],
      warnings: [],
    };
    const expectedDraft = JSON.parse(JSON.stringify(config));
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    expect(
      await screen.findByRole("region", {
        name: "deepseek-v4-flash 子 Agent 配置",
      }),
    ).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("LEGACY_ALIAS_SENTINEL");
    await user.click(
      await screen.findByRole("button", {
        name: "从模型目录恢复全部无效能力配置（1 项）",
      }),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reconcile_codex_subagent_v2_profiles",
        {
          providerId: "router",
          action: "recover_all_invalid_from_catalog",
          subagentV2: expectedDraft,
        },
      ),
    );
    const recovered = await openAdvancedFields(user);
    expect(within(recovered).getByLabelText("角色名称")).toHaveValue(
      "keep-valid-role",
    );
    expect(within(recovered).getByLabelText("角色描述")).toHaveValue(
      "KEEP_VALID_DESCRIPTION",
    );
    expect(
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles[
        "deepseek-v4-flash"
      ],
    ).toEqual(legacyProfile);
    expect(document.body.textContent).not.toContain("LEGACY_ALIAS_SENTINEL");
  });
});

describe("Codex Sub-Agent V2 searchable Accordion workspace", () => {
  it("sorts enabled routable profiles first and expands only one Accordion model", async () => {
    const user = userEvent.setup();
    await renderWorkspace();

    const triggers = await screen.findAllByRole("button", {
      name: /配置 deepseek-v4-/i,
    });
    expect(
      triggers.map((button) => button.getAttribute("aria-expanded")),
    ).toEqual(["true", "false"]);
    expect(triggers[0]).toHaveTextContent("deepseek-v4-flash");
    expect(within(triggers[0]).getByText("第三方")).toBeVisible();
    expect(within(triggers[0]).getByText("可路由")).toBeVisible();
    expect(within(triggers[0]).getByText("可用")).toBeVisible();
    expect(within(triggers[0]).getByText("推理 medium")).toBeVisible();
    expect(within(triggers[0]).getByText("含手工覆盖")).toBeVisible();
    expect(within(triggers[0]).getByText("仓库探索")).toBeVisible();
    expect(
      screen.getByRole("switch", {
        name: "启用 deepseek-v4-flash 作为 V2 子 Agent",
      }),
    ).toBeChecked();

    await user.click(triggers[1]);

    expect(triggers[0]).toHaveAttribute("aria-expanded", "false");
    expect(triggers[1]).toHaveAttribute("aria-expanded", "true");
    expect(within(triggers[1]).getByText("不可路由")).toBeVisible();
    expect(within(triggers[1]).getByText("后备")).toBeVisible();
    expect(within(triggers[1]).getByText("复杂实现")).toBeVisible();
  });

  it("supports Accordion keyboard navigation without coupling the adjacent model Switch", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const triggers = await screen.findAllByRole("button", {
      name: /配置 deepseek-v4-/i,
    });
    const flashSwitch = screen.getByRole("switch", {
      name: "启用 deepseek-v4-flash 作为 V2 子 Agent",
    });
    const proSwitch = screen.getByRole("switch", {
      name: "启用 deepseek-v4-pro 作为 V2 子 Agent",
    });

    expect(triggers[0]).toHaveAttribute("aria-controls");
    expect(triggers[1]).toHaveAttribute("aria-controls");
    expect(triggers[0].getAttribute("aria-controls")).not.toBe(
      triggers[1].getAttribute("aria-controls"),
    );
    expect(flashSwitch).toHaveAccessibleName(
      "启用 deepseek-v4-flash 作为 V2 子 Agent",
    );
    expect(proSwitch).toHaveAccessibleName(
      "启用 deepseek-v4-pro 作为 V2 子 Agent",
    );

    triggers[0].focus();
    await user.keyboard("{ArrowDown}");
    expect(triggers[1]).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(triggers[0]).toHaveAttribute("aria-expanded", "false");
    expect(triggers[1]).toHaveAttribute("aria-expanded", "true");

    expect(
      screen.getByRole("button", { name: "编辑 deepseek-v4-pro" }),
    ).toBeDisabled();
    await user.tab();
    expect(proSwitch).toHaveFocus();
    expect(proSwitch).toBeChecked();
    await user.keyboard(" ");
    expect(proSwitch).not.toBeChecked();
    expect(triggers[1]).toHaveAttribute("aria-expanded", "true");
  });

  it("通过搜索模型、profile key 和 Provider 类型快速定位配置", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const search = await screen.findByRole("searchbox", {
      name: "搜索子 Agent 模型",
    });

    await user.type(search, "offline-writer");
    expect(
      screen.getByRole("button", { name: /配置 deepseek-v4-pro/i }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /配置 deepseek-v4-flash/i }),
    ).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "third_party");
    expect(
      screen.getAllByRole("button", { name: /配置 deepseek-v4-/i }),
    ).toHaveLength(2);
  });

  it("使用筛选区分已启用、待配置、不可路由并可恢复全部", async () => {
    seedPersistedPlan(true);
    ipcState.providers.router.settingsConfig.codexRouting.subagentV2.profiles[
      "offline-writer"
    ].enabled = false;
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    const draftFilter = await screen.findByRole("button", { name: "待配置" });
    await user.click(draftFilter);
    expect(draftFilter).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: /配置 deepseek-v4-pro/i }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /配置 deepseek-v4-flash/i }),
    ).not.toBeInTheDocument();

    const unroutableFilter = screen.getByRole("button", { name: "不可路由" });
    await user.click(unroutableFilter);
    expect(unroutableFilter).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: /配置 deepseek-v4-pro/i }),
    ).toBeVisible();

    const enabledFilter = screen.getByRole("button", { name: "已启用" });
    await user.click(enabledFilter);
    expect(
      screen.getByRole("button", { name: /配置 deepseek-v4-flash/i }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /配置 deepseek-v4-pro/i }),
    ).not.toBeInTheDocument();

    const allFilter = screen.getByRole("button", { name: "全部" });
    await user.click(allFilter);
    expect(allFilter).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getAllByRole("button", { name: /配置 deepseek-v4-/i }),
    ).toHaveLength(2);
  });

  it("在搜索和筛选无结果时提供可执行的清除入口", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await user.type(
      await screen.findByRole("searchbox", { name: "搜索子 Agent 模型" }),
      "model-does-not-exist",
    );

    expect(screen.getByText("没有符合条件的子 Agent 模型")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "清除筛选" }));
    expect(
      screen.getAllByRole("button", { name: /配置 deepseek-v4-/i }),
    ).toHaveLength(2);
  });

  it("keeps invalid profiles in the repair area while normal models are filtered", async () => {
    seedMalformedProfiles();
    const user = userEvent.setup();
    await mountWorkspaceFromPersistedPlan();

    await user.type(
      await screen.findByRole("searchbox", { name: "搜索子 Agent 模型" }),
      "model-does-not-exist",
    );

    expect(screen.getByText("没有符合条件的子 Agent 模型")).toBeVisible();
    expect(
      screen.getByRole("region", { name: "无效能力配置 1" }),
    ).toBeVisible();
    expect(
      screen.getByRole("region", { name: "无效能力配置 2" }),
    ).toBeVisible();
    expect(document.body.textContent).not.toContain(
      "RAW_INVALID_PROFILE_KEY_ALPHA",
    );
    expect(document.body.textContent).not.toContain(
      "RAW_INVALID_PROFILE_KEY_BETA",
    );
  });

  it("keeps 高级字段 and generated TOML collapsed until requested", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const flash = flashRegion();

    expect(
      within(flash).getByRole("group", { name: "任务优势" }),
    ).toBeVisible();
    expect(within(flash).queryByLabelText("角色描述")).not.toBeInTheDocument();
    expect(
      within(flash).queryByText(previewFixture.tomlPreview, {
        normalizer: getDefaultNormalizer({
          trim: false,
          collapseWhitespace: false,
        }),
      }),
    ).not.toBeInTheDocument();

    await openAdvancedFields(user);
    expect(within(flash).getByLabelText("角色描述")).toBeVisible();
    await openGeneratedOutput(user);
    expect(
      within(flash).getByText(previewFixture.tomlPreview, {
        normalizer: getDefaultNormalizer({
          trim: false,
          collapseWhitespace: false,
        }),
      }),
    ).toBeVisible();
  });

  it("用清晰的模型目录动作保留完整未保存草稿", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("优化目标"),
      "质量",
    );
    const expectedUnsavedDraft = JSON.parse(
      JSON.stringify(
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2,
      ),
    );
    expectedUnsavedDraft.profiles[
      "repository-scout"
    ].questionnaire.optimization = "quality";

    expect(
      screen.getByText(
        "只加入当前可路由的第三方模型；新模型默认关闭，已有问卷和手工设置不会被覆盖。",
      ),
    ).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "添加第三方可配置模型" }),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reconcile_codex_subagent_v2_profiles",
        {
          providerId: "router",
          action: "sync_catalog",
          subagentV2: expectedUnsavedDraft,
        },
      ),
    );
    expect(
      await screen.findByText(
        "已添加第三方可配置模型；已有设置保持不变；数据库与 Codex Agent 文件均已写入并回读验证；重启 Codex/app-server 并新建会话后生效",
      ),
    ).toBeVisible();
  });

  it("reports a legal inactive/not_required reconcile without claiming role files were applied", async () => {
    ipcState.nextProjection = { status: "not_required" };
    const user = userEvent.setup();
    await renderWorkspace();

    await user.click(
      screen.getByRole("button", { name: "添加第三方可配置模型" }),
    );

    expect(
      await screen.findByText(
        "已添加第三方可配置模型；已有设置保持不变；数据库已保存；当前方案未激活，因此未改写 Codex Agent 文件",
      ),
    ).toBeVisible();
    expect(screen.queryByText(/Codex Agent 文件均已写入并回读验证/)).toBeNull();
  });

  it("在粘性保存区反馈未保存和保存成功状态", async () => {
    const user = userEvent.setup();
    await renderWorkspace();

    expect(screen.getByText("所有更改均已保存")).toBeVisible();
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("优化目标"),
      "质量",
    );
    expect(screen.getByText("有未保存更改")).toBeVisible();

    await saveV2(user);

    expect(
      await screen.findByText(
        "数据库与 Codex Agent 文件均已写入并回读验证；重启 Codex/app-server 并新建会话后生效",
      ),
    ).toBeVisible();
    expect(screen.getByText("所有更改均已保存")).toBeVisible();
  });

  it("does not report success when the backend omits write verification", async () => {
    ipcState.omitWriteVerification = true;
    const user = userEvent.setup();
    await renderWorkspace();
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("优化目标"),
      "质量",
    );

    await saveV2(user);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "后端未返回 Codex Agent 文件写入验证结果",
    );
    expect(screen.queryByText(/Codex Agent 文件均已写入并回读验证/)).toBeNull();
  });

  it.each([
    ["missing", undefined],
    ["incorrect", "already_active"],
  ])(
    "does not report ordinary save success when verification.activation is %s",
    async (_label, activation) => {
      ipcState.nextVerification = appliedVerification({ activation });
      const user = userEvent.setup();
      await renderWorkspace();
      await chooseOption(
        user,
        within(flashRegion()).getByLabelText("优化目标"),
        "质量",
      );

      await saveV2(user);

      expect(
        await screen.findByText("后端返回的 Codex Agent 激活条件不一致。"),
      ).toBeVisible();
      expect(
        screen.queryByText(/Codex Agent 文件均已写入并回读验证/),
      ).toBeNull();
    },
  );

  it("persists explicit text and image input capability in the V2 profile JSON", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const capability = within(flashRegion()).getByLabelText("输入能力");

    await chooseOption(user, capability, "文本与图像");
    await saveV2(user);

    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].inputModalities,
      ).toEqual(["text", "image"]),
    );
  });
});

describe("Codex Sub-Agent V2 shared editor accessible areas", () => {
  it("shows catalog-derived text-only capability before the legacy profile is resaved", async () => {
    seedPersistedPlan(true);
    ipcState.providers["third-party"].settingsConfig.modelCatalog.models[0] = {
      ...ipcState.providers["third-party"].settingsConfig.modelCatalog
        .models[0],
      inputModalities: ["text"],
      textOnly: true,
      supportsImage: false,
    };
    await mountWorkspaceFromPersistedPlan();

    expect(within(flashRegion()).getByLabelText("输入能力")).toHaveValue(
      "text_only",
    );
  });

  it("disables editing for an unavailable model and keeps routable models editable", async () => {
    const user = userEvent.setup();
    await renderWorkspace();

    expect(
      screen.getByRole("button", { name: "编辑 deepseek-v4-pro" }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: "编辑 deepseek-v4-flash" }),
    );

    expect(
      screen.getByRole("region", {
        name: "deepseek-v4-flash 子 Agent 配置",
      }),
    ).toBeVisible();
  });

  it("keeps the questionnaire visible and advanced areas collapsed", async () => {
    const user = userEvent.setup();
    await renderWorkspace();

    expect(
      await screen.findByRole("heading", { name: "选择策略" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "配置 deepseek-v4-flash" }),
    ).toHaveAttribute("aria-expanded", "true");
    const flash = flashRegion();
    expect(
      within(flash).getByRole("group", { name: "任务优势" }),
    ).toBeVisible();
    const advanced = within(flash).getByRole("button", { name: "高级字段" });
    const generated = within(flash).getByRole("button", {
      name: "生成结果与 TOML",
    });
    expect(advanced).toHaveAttribute("aria-expanded", "false");
    expect(generated).toHaveAttribute("aria-expanded", "false");

    await user.click(advanced);
    expect(within(flash).getByLabelText("角色描述")).toBeVisible();
    await user.click(generated);
    expect(within(flashBackendRegion()).getByText("请求角色名")).toBeVisible();
  });
});

describe("Codex Sub-Agent V2 dual-theme visual contract", () => {
  it("uses the MultiRouter blue, violet, and cyan hierarchy in both themes", async () => {
    await renderWorkspace();

    const editor = document.querySelector<HTMLElement>(
      '[data-theme-contract="codex-subagent-v2"]',
    );
    if (!editor) throw new Error("missing V2 theme contract root");
    expect(editor).toHaveClass(
      "from-blue-50/80",
      "to-violet-50/70",
      "dark:from-blue-950/25",
      "dark:to-violet-950/20",
    );

    const strategy = editor.querySelector<HTMLElement>(
      '[data-subagent-panel="strategy"]',
    );
    const catalog = editor.querySelector<HTMLElement>(
      '[data-subagent-panel="catalog"]',
    );
    expect(strategy).toHaveClass(
      "border-blue-200",
      "bg-blue-50/70",
      "dark:border-blue-500/40",
      "dark:bg-blue-950/20",
    );
    expect(catalog).toHaveClass(
      "border-cyan-200/70",
      "bg-cyan-50/60",
      "dark:border-cyan-500/30",
      "dark:bg-cyan-950/15",
    );
  });

  it("assigns semantic paired colors to routable and unroutable model cards", async () => {
    await renderWorkspace();

    const flashTrigger = await screen.findByRole("button", {
      name: "配置 deepseek-v4-flash",
    });
    const proTrigger = screen.getByRole("button", {
      name: "配置 deepseek-v4-pro",
    });
    const flashCard = flashTrigger.closest<HTMLElement>(
      '[data-profile-tone="enabled-routable"]',
    );
    const proCard = proTrigger.closest<HTMLElement>(
      '[data-profile-tone="unroutable"]',
    );
    expect(flashCard).toHaveClass(
      "border-emerald-200",
      "bg-emerald-50/70",
      "dark:border-emerald-500/40",
      "dark:bg-emerald-950/25",
    );
    expect(proCard).toHaveClass(
      "border-border/70",
      "bg-muted/35",
      "dark:border-slate-700/70",
      "dark:bg-slate-900/35",
    );
  });

  it("renders questionnaire choices as dual-theme capability chips", async () => {
    await renderWorkspace();
    const flash = flashRegion();
    const selectedChip = within(flash)
      .getByRole("checkbox", { name: "仓库探索" })
      .closest("label");
    const idleChip = within(flash)
      .getByRole("checkbox", { name: "复杂调试" })
      .closest("label");

    expect(selectedChip).toHaveClass(
      "border-sky-300",
      "bg-sky-100/80",
      "text-sky-950",
      "dark:border-sky-500/50",
      "dark:bg-sky-500/15",
      "dark:text-sky-100",
    );
    expect(idleChip).toHaveClass("bg-background/70", "dark:bg-slate-950/35");
  });

  it("distinguishes advanced, TOML, saved, and dirty states by paired colors", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const flash = flashRegion();
    expect(within(flash).getByRole("button", { name: "高级字段" })).toHaveClass(
      "border-violet-200",
      "bg-violet-50",
      "dark:border-violet-500/40",
      "dark:bg-violet-950/25",
    );
    expect(
      within(flash).getByRole("button", { name: "生成结果与 TOML" }),
    ).toHaveClass(
      "border-cyan-200",
      "bg-cyan-50",
      "dark:border-cyan-500/40",
      "dark:bg-cyan-950/25",
    );

    const savedBar = document.querySelector<HTMLElement>(
      '[data-save-state="saved"]',
    );
    expect(savedBar).toHaveClass(
      "border-emerald-200",
      "bg-emerald-50/95",
      "dark:border-emerald-500/40",
      "dark:bg-emerald-950/90",
    );

    await chooseOption(user, within(flash).getByLabelText("优化目标"), "质量");
    const dirtyBar = document.querySelector<HTMLElement>(
      '[data-save-state="dirty"]',
    );
    expect(dirtyBar).toHaveClass(
      "border-blue-200",
      "bg-blue-50/95",
      "dark:border-blue-500/40",
      "dark:bg-blue-950/90",
    );
  });
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
      if (optionName === "均衡") {
        seedPersistedPlan(true);
        ipcState.providers.router.settingsConfig.codexRouting.subagentV2.selectionPolicy =
          "official_first";
        await mountWorkspaceFromPersistedPlan();
      } else {
        await renderWorkspace();
      }
      const policy = await screen.findByLabelText("第三方子 Agent 选择策略");
      await chooseOption(user, policy, optionName);
      await saveV2(user);
      await expectSavedSettingsConfig({
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          routes: [currentV2Route()],
          subagentV2: {
            schemaVersion: 2,
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
                },
                reasoning: { policy: "fixed", effort: "medium" },
                overrides: {
                  roleName: "repository-scout",
                  developerInstructions: "Do not modify source files.",
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
                },
                reasoning: { policy: "fixed", effort: "high" },
              },
            },
          },
        },
      });
    },
  );

  it("changes only the selected model enabled flag in the full save payload", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await user.click(
      screen.getByRole("switch", {
        name: "启用 deepseek-v4-flash 作为 V2 子 Agent",
      }),
    );
    await saveV2(user);
    await expectSavedSettingsConfig({
      codexRouting: {
        schemaVersion: 2,
        enabled: true,
        routes: [currentV2Route()],
        subagentV2: {
          schemaVersion: 2,
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
              },
              reasoning: { policy: "fixed", effort: "medium" },
              overrides: {
                roleName: "repository-scout",
                developerInstructions: "Do not modify source files.",
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
              },
              reasoning: { policy: "fixed", effort: "high" },
            },
          },
        },
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
      schemaVersion: 2,
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
          },
          reasoning: { policy: "fixed", effort: "medium" },
          overrides: {
            roleName: "repository-scout",
            developerInstructions: "Do not modify source files.",
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
          },
          reasoning: { policy: "fixed", effort: "high" },
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
    await chooseOption(
      user,
      screen.getByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );
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
      },
    ],
  ])(
    "saves %s=%s without changing questionnaire siblings",
    async (label, value, alternate, expectedQuestionnaire) => {
      const user = userEvent.setup();
      await renderWorkspace();
      const control = within(flashRegion()).getByLabelText(label);
      const startedAtRequestedValue =
        (control instanceof HTMLSelectElement && control.value === value) ||
        (!(control instanceof HTMLSelectElement) &&
          control.textContent?.includes(value));
      if (startedAtRequestedValue) {
        await chooseOption(user, control, alternate);
      }
      await chooseOption(user, control, value);
      if (startedAtRequestedValue) {
        await chooseOption(
          user,
          screen.getByLabelText("第三方子 Agent 选择策略"),
          "官方优先",
        );
      }
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
    ["delegated", { policy: "delegated" }],
    ["model_default", { policy: "model_default" }],
    ["disabled", { policy: "disabled" }],
  ] as const)(
    "persists reasoning policy %s outside the questionnaire",
    async (policy, expected) => {
      const user = userEvent.setup();
      await renderWorkspace();
      await chooseOption(
        user,
        within(flashRegion()).getByLabelText("推理策略"),
        policy,
      );
      await saveV2(user);
      await waitFor(() =>
        expect(
          latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
            "repository-scout"
          ].reasoning,
        ).toEqual(expected),
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
      const input = within(await openAdvancedFields(user)).getByLabelText(
        label,
      );
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
        }),
      );
    },
  );

  it("edits model reasoning effort and preserves all other overrides", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await openAdvancedFields(user);
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
      }),
    );
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
        "repository-scout"
      ].reasoning,
    ).toEqual({ policy: "fixed", effort: "high" });
  });

  it("persists max and exposes it in the generated role preview", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await openAdvancedFields(user);
    await chooseOption(
      user,
      within(flashRegion()).getByLabelText("模型推理强度"),
      "max",
    );
    const flash = within(flashRegion());
    await user.click(flash.getByRole("button", { name: "生成结果与 TOML" }));
    expect(
      await flash.findByText(/model_reasoning_effort = "max"/),
    ).toBeInTheDocument();
    await saveV2(user);
    await waitFor(() =>
      expect(
        latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
          "repository-scout"
        ].reasoning,
      ).toEqual({ policy: "fixed", effort: "max" }),
    );
  });

  it("uses backend capability evidence to restrict DeepSeek fixed effort choices", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const flash = within(await openAdvancedFields(user));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "get_codex_subagent_reasoning_capabilities",
        expect.objectContaining({ settingsConfig: expect.any(Object) }),
      ),
    );
    expect(
      flash.getByText("能力来源：builtin（confirmed）"),
    ).toBeInTheDocument();
    expect(
      flash.getByText("Provider 原生档位：low / high / max"),
    ).toBeInTheDocument();
    expect(
      flash.getByText(
        "Codex 可选档位：none / low / medium / high / xhigh / max",
      ),
    ).toBeInTheDocument();
    expect(
      flash.getByText(
        "映射：low→low，medium→high，high→high，xhigh→high，max→max",
      ),
    ).toBeInTheDocument();

    const effort = flash.getByLabelText("模型推理强度");
    expect(
      within(effort)
        .getAllByRole("option")
        .map((option) => option.getAttribute("value")),
    ).toEqual(["low", "medium", "high", "xhigh", "max"]);
  });

  it("makes delegated, model-default, fixed and disabled semantics explicit", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const flash = within(await openProfile(user, "deepseek-v4-flash"));
    const policy = flash.getByLabelText("推理策略");

    await chooseOption(user, policy, "允许主 Agent / spawn 指定");
    expect(
      await flash.findByText("由主 Agent、spawn 参数或 Codex 默认值决定"),
    ).toBeInTheDocument();

    await chooseOption(user, policy, "使用模型默认（固定）");
    expect(
      await flash.findByText("固定为模型当前默认 high"),
    ).toBeInTheDocument();

    await chooseOption(user, policy, "关闭推理");
    expect(
      await flash.findByText("关闭推理；上游将使用已确认的 Provider 关闭语义"),
    ).toBeInTheDocument();
  });

  it("removes description alone after all five overrides were established", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    const region = within(await openAdvancedFields(user));
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
      }),
    );
    expect(
      latestSavedPlan()?.settingsConfig.codexRouting.subagentV2.profiles[
        "repository-scout"
      ].reasoning,
    ).toEqual({ policy: "fixed", effort: "high" });
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
    await openAdvancedFields(user);
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
    await openAdvancedFields(user);
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

  it("shares the workspace-saved V2 source with the remounted workspace", async () => {
    const workspace = await renderPersistedWorkspace();
    await openAdvancedFields(workspace.user);
    await chooseOption(
      workspace.user,
      await screen.findByLabelText("第三方子 Agent 选择策略"),
      "官方优先",
    );
    await chooseOption(
      workspace.user,
      within(flashRegion()).getByLabelText("模型偏好"),
      "preferred",
    );
    await chooseOption(
      workspace.user,
      within(flashRegion()).getByLabelText("优化目标"),
      "质量",
    );
    const roleName = within(flashRegion()).getByLabelText("角色名称");
    await workspace.user.clear(roleName);
    await workspace.user.type(roleName, "workspace-scout");
    await saveV2(workspace.user);
    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
    workspace.unmount();

    await mountWorkspaceFromPersistedPlan();
    await openAdvancedFields(workspace.user);
    expectControlValue(
      await screen.findByLabelText("第三方子 Agent 选择策略"),
      "official_first",
      "官方优先",
    );
    expectControlValue(
      within(flashRegion()).getByLabelText("模型偏好"),
      "preferred",
    );
    expectControlValue(
      within(flashRegion()).getByLabelText("优化目标"),
      "quality",
    );
    expect(within(flashRegion()).getByLabelText("角色名称")).toHaveValue(
      "workspace-scout",
    );
  });

  it("keeps a pending projection warning visible after the workspace persisted callback refreshes provider props", async () => {
    const warning = "数据库已保存，Codex live 投影待重试。";
    ipcState.nextProjection = {
      status: "pending_retry",
      warning: {
        code: "codex_live_projection_pending_retry",
        message: warning,
      },
    };
    const workspace = await renderPersistedWorkspace();
    await openAdvancedFields(workspace.user);
    const description = within(flashRegion()).getByLabelText("角色描述");
    await workspace.user.clear(description);
    await workspace.user.type(description, "warning lifecycle refresh draft");

    await saveV2(workspace.user);
    await waitFor(() => expect(v2PersistenceCalls()).toHaveLength(1));
    await new Promise((resolve) => setTimeout(resolve, 25));

    expect(screen.getByText(warning)).toBeInTheDocument();
    expect(
      screen.getByText("数据库已保存；Codex Agent 文件投影待自动重试"),
    ).toBeInTheDocument();
    expect(within(flashRegion()).getByLabelText("角色描述")).toHaveValue(
      "warning lifecycle refresh draft",
    );
  });

  it("initializes V2 with one backend-owned focused mutation", async () => {
    const user = userEvent.setup();
    await renderWorkspace(false);
    await user.click(
      await screen.findByRole("button", {
        name: "初始化 V2 子 Agent 能力配置",
      }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("initialize_codex_subagent_v2", {
        providerId: "router",
      }),
    );
    expect(
      await screen.findByText(
        "V2 子 Agent 能力配置已初始化；数据库与 Codex Agent 文件均已写入并回读验证；重启 Codex/app-server 并新建会话后生效",
      ),
    ).toBeVisible();
    expect(v2PersistenceCalls()).toHaveLength(0);
    expect(
      ipcState.providers.router.settingsConfig.codexRouting.subagentV2,
    ).toEqual({
      schemaVersion: 2,
      selectionPolicy: "balanced",
      profiles: {
        "qwen-draft": {
          model: "QWEN-ＤＲＡＦＴ",
          enabled: false,
          questionnaire: {
            taskStrengths: ["repository_exploration"],
            optimization: "balanced",
            writeScope: "read_only",
            preference: "eligible",
          },
          reasoning: { policy: "delegated" },
        },
      },
    });
  });
});

describe("Codex Sub-Agent V2 preview visible output", () => {
  it("distinguishes requested and effective role names", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await openGeneratedOutput(user);
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
    const user = userEvent.setup();
    await renderWorkspace();
    await openGeneratedOutput(user);
    expect(
      (await within(flashBackendRegion()).findAllByText(value)).length,
    ).toBeGreaterThan(0);
  });

  it("renders backend-returned backend TOML", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await openGeneratedOutput(user);
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
          },
          reasoning: { policy: "fixed", effort: "medium" },
          overrides: {
            roleName: "repository-scout",
            developerInstructions: "Do not modify source files.",
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
    const user = userEvent.setup();
    await renderWorkspace();
    if (scope === "flash") await openGeneratedOutput(user);
    if (scope === "pro") {
      await openGeneratedOutput(user, "deepseek-v4-pro");
    }
    const output =
      scope === "flash"
        ? within(flashBackendRegion())
        : scope === "pro"
          ? within(proBackendRegion())
          : screen;
    expect(await output.findByText(value)).toBeInTheDocument();
  });

  it("renders authoritative requested and effective roles separately", async () => {
    const user = userEvent.setup();
    await renderWorkspace();
    await openGeneratedOutput(user);
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
