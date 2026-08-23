import { http, HttpResponse } from "msw";
import type { AppId } from "@/lib/api/types";
import type { McpServer, Provider, Settings } from "@/types";
import type {
  CodexSubagentProfilePreview,
  CodexSubagentProfileStatuses,
  CodexSubagentReasoningCapability,
} from "@/types/codexSubagentV2";
import {
  addProvider,
  deleteProvider,
  deleteSession,
  getCurrentProviderId,
  getLiveProviderIds,
  getSessionMessages,
  getProviders,
  listProviders,
  listSessions,
  resetProviderState,
  setCurrentProviderId,
  updateProvider,
  updateSortOrder,
  getSettings,
  setSettings,
  getAppConfigDirOverride,
  setAppConfigDirOverrideState,
  getMcpConfig,
  setMcpServerEnabled,
  upsertMcpServer,
  deleteMcpServer,
} from "./state";

const TAURI_ENDPOINT = "http://tauri.local";

const withJson = async <T>(request: Request): Promise<T> => {
  try {
    const body = await request.text();
    if (!body) return {} as T;
    return JSON.parse(body) as T;
  } catch {
    return {} as T;
  }
};

const success = <T>(payload: T) => HttpResponse.json(payload as any);

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

const codexSubagentPreviewFixtures: Record<
  string,
  CodexSubagentProfilePreview
> = {
  "deepseek-v4-flash": {
    providerKind: "third_party",
    requestedRoleName: "deepseek-flash",
    effectiveRoleName: "deepseek-flash",
    description: "Read repositories, trace architecture, and collect evidence.",
    developerInstructions:
      "Inspect the repository carefully and do not modify source files.",
    nicknameCandidates: ["Flash", "Scout"],
    model: "deepseek-v4-flash",
    modelProvider: "codex_model_router_v2",
    modelReasoningEffort: "medium",
    reasoningPolicy: "fixed",
    reasoningCapability: deepseekReasoningCapability,
    modelContextWindow: 128000,
    tomlPreview:
      '[agents.deepseek-flash]\nmodel = "deepseek-v4-flash"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "medium"',
    warnings: [],
  },
  "deepseek-v4-pro": {
    providerKind: "third_party",
    requestedRoleName: "deepseek-pro",
    effectiveRoleName: "deepseek-pro",
    description:
      "Handle complex debugging, architecture decisions, and high-risk review.",
    developerInstructions:
      "Investigate the complete call chain before proposing or making changes.",
    nicknameCandidates: ["Pro", "Reviewer"],
    model: "deepseek-v4-pro",
    modelProvider: "codex_model_router_v2",
    modelReasoningEffort: "high",
    reasoningPolicy: "fixed",
    reasoningCapability: deepseekReasoningCapability,
    modelContextWindow: 128000,
    tomlPreview:
      '[agents.deepseek-pro]\nmodel = "deepseek-v4-pro"\nmodel_provider = "codex_model_router_v2"\nmodel_reasoning_effort = "high"',
    warnings: [],
  },
};

const codexSubagentStatusesFixture: CodexSubagentProfileStatuses = {
  mode: "v2",
  generationSource: "configured_profiles",
  profiles: [
    {
      profileKey: "deepseek-v4-flash",
      model: "deepseek-v4-flash",
      providerKind: "third_party",
      enabled: true,
      routable: true,
      fieldSources: {
        roleName: "automatic",
        description: "automatic",
        developerInstructions: "automatic",
        nicknameCandidates: "automatic",
        modelReasoningEffort: "automatic",
      },
      requestedRoleName: "deepseek-flash",
      effectiveRoleName: "deepseek-flash",
      roleFilePath: "C:\\Codex\\agents\\deepseek-flash.toml",
      modelProvider: "codex_model_router_v2",
      modelReasoningEffort: "medium",
      status: "generated",
      warnings: [],
    },
    {
      profileKey: "deepseek-v4-pro",
      model: "deepseek-v4-pro",
      providerKind: "third_party",
      enabled: true,
      routable: false,
      status: "unroutable",
      nonGenerationReason: "unroutable",
      warnings: ["No enabled route resolves this model."],
    },
  ],
  warnings: ["One profile is unroutable."],
};

export const handlers = [
  http.post(`${TAURI_ENDPOINT}/get_migration_result`, () => success(false)),
  http.post(`${TAURI_ENDPOINT}/get_skills_migration_result`, () =>
    success(null),
  ),
  http.post(`${TAURI_ENDPOINT}/list_profiles`, () => success([])),
  http.post(`${TAURI_ENDPOINT}/get_providers`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(getProviders(app));
  }),

  http.post(`${TAURI_ENDPOINT}/get_current_provider`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(getCurrentProviderId(app));
  }),

  http.post(
    `${TAURI_ENDPOINT}/inspect_active_codex_multirouter_projection`,
    () => success(null),
  ),

  http.post(
    `${TAURI_ENDPOINT}/update_providers_sort_order`,
    async ({ request }) => {
      const { updates = [], app } = await withJson<{
        updates: { id: string; sortIndex: number }[];
        app: AppId;
      }>(request);
      updateSortOrder(app, updates);
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/update_tray_menu`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/get_opencode_live_provider_ids`, () =>
    success(getLiveProviderIds("opencode")),
  ),

  http.post(`${TAURI_ENDPOINT}/get_openclaw_live_provider_ids`, () =>
    success(getLiveProviderIds("openclaw")),
  ),

  http.post(`${TAURI_ENDPOINT}/get_openclaw_default_model`, () =>
    success({ primary: null, fallback: [] }),
  ),

  http.post(`${TAURI_ENDPOINT}/scan_openclaw_config_health`, () => success([])),

  http.post(`${TAURI_ENDPOINT}/switch_provider`, async ({ request }) => {
    const { id, app } = await withJson<{ id: string; app: AppId }>(request);
    const providers = listProviders(app);
    if (!providers[id]) {
      return HttpResponse.json(false, { status: 404 });
    }
    setCurrentProviderId(app, id);
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/add_provider`, async ({ request }) => {
    const { provider, app } = await withJson<{
      provider: Provider & { id?: string };
      app: AppId;
    }>(request);

    const newId = provider.id ?? `mock-${Date.now()}`;
    addProvider(app, { ...provider, id: newId });
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/update_provider`, async ({ request }) => {
    const { provider, app } = await withJson<{
      provider: Provider;
      app: AppId;
    }>(request);
    updateProvider(app, provider);
    return success(true);
  }),

  http.post(
    `${TAURI_ENDPOINT}/preview_codex_subagent_profile`,
    async ({ request }) => {
      const { model } = await withJson<{ model: string }>(request);
      return success(
        codexSubagentPreviewFixtures[model] ??
          codexSubagentPreviewFixtures["deepseek-v4-flash"],
      );
    },
  ),

  http.post(`${TAURI_ENDPOINT}/get_codex_subagent_profile_statuses`, () =>
    success(codexSubagentStatusesFixture),
  ),

  http.post(`${TAURI_ENDPOINT}/delete_provider`, async ({ request }) => {
    const { id, app } = await withJson<{ id: string; app: AppId }>(request);
    deleteProvider(app, id);
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/import_default_config`, async () => {
    resetProviderState();
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/open_external`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/list_sessions`, () => success(listSessions())),

  http.post(`${TAURI_ENDPOINT}/get_session_messages`, async ({ request }) => {
    const { providerId, sourcePath } = await withJson<{
      providerId: string;
      sourcePath: string;
    }>(request);
    return success(getSessionMessages(providerId, sourcePath));
  }),

  http.post(`${TAURI_ENDPOINT}/delete_session`, async ({ request }) => {
    const { providerId, sessionId, sourcePath } = await withJson<{
      providerId: string;
      sessionId: string;
      sourcePath: string;
    }>(request);
    return success(deleteSession(providerId, sessionId, sourcePath));
  }),

  http.post(`${TAURI_ENDPOINT}/delete_sessions`, async ({ request }) => {
    const { items = [] } = await withJson<{
      items?: {
        providerId: string;
        sessionId: string;
        sourcePath: string;
      }[];
    }>(request);

    return success(
      items.map((item) => ({
        providerId: item.providerId,
        sessionId: item.sessionId,
        sourcePath: item.sourcePath,
        success: deleteSession(
          item.providerId,
          item.sessionId,
          item.sourcePath,
        ),
      })),
    );
  }),

  // MCP APIs
  http.post(`${TAURI_ENDPOINT}/get_mcp_config`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(getMcpConfig(app));
  }),

  http.post(`${TAURI_ENDPOINT}/import_mcp_from_claude`, () => success(1)),
  http.post(`${TAURI_ENDPOINT}/import_mcp_from_codex`, () => success(1)),

  http.post(`${TAURI_ENDPOINT}/set_mcp_enabled`, async ({ request }) => {
    const { app, id, enabled } = await withJson<{
      app: AppId;
      id: string;
      enabled: boolean;
    }>(request);
    setMcpServerEnabled(app, id, enabled);
    return success(true);
  }),

  http.post(
    `${TAURI_ENDPOINT}/upsert_mcp_server_in_config`,
    async ({ request }) => {
      const { app, id, spec } = await withJson<{
        app: AppId;
        id: string;
        spec: McpServer;
      }>(request);
      upsertMcpServer(app, id, spec);
      return success(true);
    },
  ),

  http.post(
    `${TAURI_ENDPOINT}/delete_mcp_server_in_config`,
    async ({ request }) => {
      const { app, id } = await withJson<{ app: AppId; id: string }>(request);
      deleteMcpServer(app, id);
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/restart_app`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/get_settings`, () => success(getSettings())),

  http.post(`${TAURI_ENDPOINT}/check_env_conflicts`, () => success([])),

  http.post(`${TAURI_ENDPOINT}/save_settings`, async ({ request }) => {
    const { settings } = await withJson<{ settings: Settings }>(request);
    setSettings(settings);
    return success(true);
  }),

  http.post(
    `${TAURI_ENDPOINT}/set_app_config_dir_override`,
    async ({ request }) => {
      const { path } = await withJson<{ path: string | null }>(request);
      setAppConfigDirOverrideState(path ?? null);
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/get_app_config_dir_override`, () =>
    success(getAppConfigDirOverride()),
  ),

  http.post(
    `${TAURI_ENDPOINT}/apply_claude_plugin_config`,
    async ({ request }) => {
      const { official } = await withJson<{ official: boolean }>(request);
      setSettings({ enableClaudePluginIntegration: !official });
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/apply_claude_onboarding_skip`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/clear_claude_onboarding_skip`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/get_config_dir`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(app === "claude" ? "/default/claude" : "/default/codex");
  }),

  http.post(`${TAURI_ENDPOINT}/is_portable_mode`, () => success(false)),

  http.post(
    `${TAURI_ENDPOINT}/select_config_directory`,
    async ({ request }) => {
      const { defaultPath, default_path } = await withJson<{
        defaultPath?: string;
        default_path?: string;
      }>(request);
      const initial = defaultPath ?? default_path;
      return success(initial ? `${initial}/picked` : "/mock/selected-dir");
    },
  ),

  http.post(`${TAURI_ENDPOINT}/pick_directory`, async ({ request }) => {
    const { defaultPath, default_path } = await withJson<{
      defaultPath?: string;
      default_path?: string;
    }>(request);
    const initial = defaultPath ?? default_path;
    return success(initial ? `${initial}/picked` : "/mock/selected-dir");
  }),

  http.post(`${TAURI_ENDPOINT}/open_file_dialog`, () =>
    success("/mock/import-settings.json"),
  ),

  http.post(
    `${TAURI_ENDPOINT}/import_config_from_file`,
    async ({ request }) => {
      const { filePath } = await withJson<{ filePath: string }>(request);
      if (!filePath) {
        return success({ success: false, message: "Missing file" });
      }
      setSettings({ language: "en" });
      return success({ success: true, backupId: "backup-123" });
    },
  ),

  http.post(`${TAURI_ENDPOINT}/export_config_to_file`, async ({ request }) => {
    const { filePath } = await withJson<{ filePath: string }>(request);
    if (!filePath) {
      return success({ success: false, message: "Invalid destination" });
    }
    return success({ success: true, filePath });
  }),

  http.post(`${TAURI_ENDPOINT}/save_file_dialog`, () =>
    success("/mock/export-settings.json"),
  ),

  // Sync current providers live (no-op success)
  http.post(`${TAURI_ENDPOINT}/sync_current_providers_live`, () =>
    success({ success: true }),
  ),

  // Proxy status (for SettingsPage / ProxyPanel hooks)
  http.post(`${TAURI_ENDPOINT}/get_proxy_status`, () =>
    success({
      running: false,
      address: "127.0.0.1",
      port: 0,
      active_connections: 0,
      total_requests: 0,
      success_requests: 0,
      failed_requests: 0,
      success_rate: 0,
      uptime_seconds: 0,
      current_provider: null,
      current_provider_id: null,
      last_request_at: null,
      last_error: null,
      failover_count: 0,
      active_targets: [],
    }),
  ),

  http.post(`${TAURI_ENDPOINT}/get_proxy_takeover_status`, () =>
    success({
      claude: false,
      codex: false,
      gemini: false,
      grokbuild: false,
    }),
  ),

  http.post(`${TAURI_ENDPOINT}/is_live_takeover_active`, () => success(false)),

  // Failover / circuit breaker defaults
  http.post(`${TAURI_ENDPOINT}/get_failover_queue`, () => success([])),
  http.post(`${TAURI_ENDPOINT}/get_available_providers_for_failover`, () =>
    success([]),
  ),
  http.post(`${TAURI_ENDPOINT}/add_to_failover_queue`, () => success(true)),
  http.post(`${TAURI_ENDPOINT}/remove_from_failover_queue`, () =>
    success(true),
  ),
  http.post(`${TAURI_ENDPOINT}/reorder_failover_queue`, () => success(true)),
  http.post(`${TAURI_ENDPOINT}/set_failover_item_enabled`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/get_circuit_breaker_config`, () =>
    success({
      failureThreshold: 3,
      successThreshold: 2,
      timeoutSeconds: 60,
      errorRateThreshold: 50,
      minRequests: 5,
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/update_circuit_breaker_config`, () =>
    success(true),
  ),
  http.post(`${TAURI_ENDPOINT}/get_provider_health`, () =>
    success({
      provider_id: "mock-provider",
      app_type: "claude",
      is_healthy: true,
      consecutive_failures: 0,
      last_success_at: null,
      last_failure_at: null,
      last_error: null,
      updated_at: new Date().toISOString(),
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/reset_circuit_breaker`, () => success(true)),
  http.post(`${TAURI_ENDPOINT}/get_circuit_breaker_stats`, () => success(null)),
];
