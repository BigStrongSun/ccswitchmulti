import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  cleanup,
  render,
  screen,
  waitFor,
  fireEvent,
} from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { providersApi } from "@/lib/api/providers";
import {
  resetProviderState,
  setCurrentProviderId,
  setLiveProviderIds,
  setProviders,
  setSettings,
} from "../msw/state";
import { server } from "../msw/server";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();
const skillsPanelMocks = vi.hoisted(() => ({
  checkUpdates: vi.fn(),
  openDiscovery: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
    onEdit,
    onDuplicate,
    onConfigureUsage,
    onOpenWebsite,
    onCreate,
    onStartCodexMultiRouterWizard,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
      <button onClick={() => onEdit(providers[currentProviderId])}>edit</button>
      <button onClick={() => onDuplicate(providers[currentProviderId])}>
        duplicate
      </button>
      <button onClick={() => onConfigureUsage(providers[currentProviderId])}>
        usage
      </button>
      <button onClick={() => onOpenWebsite("https://example.com")}>
        open-website
      </button>
      <button onClick={() => onCreate?.()}>create</button>
      {onStartCodexMultiRouterWizard ? (
        <button onClick={() => onStartCodexMultiRouterWizard()}>
          open-multirouter-entry
        </button>
      ) : null}
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open, provider, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="edit-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              provider: {
                ...provider,
                name: `${provider.name}-edited`,
              },
              originalId: provider.id,
            })
          }
        >
          confirm-edit
        </button>
        <button onClick={() => onOpenChange(false)}>close-edit</button>
      </div>
    ) : null,
}));

vi.mock("@/components/codex/CodexRouterWorkspacePage", async () => {
  const actual = await vi.importActual<any>(
    "@/components/codex/CodexRouterWorkspacePage",
  );
  return {
    ...actual,
    CodexRouterWorkspacePage: ({
      initialProviderId,
      initialTab,
      onRuntimeReady,
    }: any) => (
      <div
        data-testid="codex-router-workspace"
        data-runtime-ready-wired={String(Boolean(onRuntimeReady))}
      >
        <span data-testid="codex-router-target">{initialProviderId}</span>
        <span data-testid="codex-router-tab">{initialTab}</span>
      </div>
    ),
  };
});

vi.mock("@/components/codex/CodexUsagePage", () => ({
  CodexUsagePage: () => <div data-testid="codex-usage-page" />,
}));

vi.mock("@/components/codex/CodexMultiRouterWizard", () => ({
  CodexMultiRouterWizard: ({
    open,
    mode,
    planId,
    newlyCreatedProviderIds,
    onOpenChange,
    onCreateProvider,
    onOpenHistoryRepair,
  }: any) =>
    open ? (
      <div
        data-testid="codex-multirouter-wizard"
        data-mode={mode}
        data-plan-id={planId ?? ""}
        data-new-provider-ids={(newlyCreatedProviderIds ?? []).join(",")}
      >
        <button onClick={() => onOpenChange(false)}>close-wizard</button>
        <button onClick={() => onCreateProvider?.()}>add-provider</button>
        <button onClick={() => onOpenHistoryRepair?.()}>
          open-history-repair
        </button>
      </div>
    ) : null,
}));

vi.mock("@/components/sessions/SessionManagerPage", () => ({
  SessionManagerPage: ({
    appId,
    initialCodexHistoryRepair,
    onInitialCodexHistoryRepairConsumed,
  }: any) => (
    <div
      data-testid="session-manager-page"
      data-app-id={appId}
      data-initial-history-repair={String(Boolean(initialCodexHistoryRepair))}
    >
      <button onClick={() => onInitialCodexHistoryRepairConsumed?.()}>
        consume-history-repair
      </button>
    </div>
  ),
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen, provider, onSave, onClose }: any) =>
    isOpen ? (
      <div data-testid="usage-modal">
        <span data-testid="usage-provider">{provider?.id}</span>
        <button onClick={() => onSave("script-code")}>save-script</button>
        <button onClick={() => onClose()}>close-usage</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/skills/UnifiedSkillsPanel", async () => {
  const React = await import("react");
  const MockUnifiedSkillsPanel = React.forwardRef(
    ({ onCheckUpdatesStateChange }: any, ref) => {
      React.useEffect(() => {
        onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: true });
        return () =>
          onCheckUpdatesStateChange?.({
            isChecking: false,
            hasSkills: false,
          });
      }, [onCheckUpdatesStateChange]);
      React.useImperativeHandle(ref, () => ({
        openDiscovery: skillsPanelMocks.openDiscovery,
        openImport: vi.fn(),
        openInstallFromZip: vi.fn(),
        openRestoreFromBackup: vi.fn(),
        checkUpdates: skillsPanelMocks.checkUpdates,
      }));
      return <div data-testid="unified-skills-panel" />;
    },
  );
  MockUnifiedSkillsPanel.displayName = "MockUnifiedSkillsPanel";
  return { default: MockUnifiedSkillsPanel };
});

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
};

describe("App integration with MSW", () => {
  // App 集成测试会打开 Radix dialog/focus guard；显式清理可避免 timeout 后污染下一条用例。
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    resetProviderState();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastWarningMock.mockReset();
    skillsPanelMocks.checkUpdates.mockReset();
    skillsPanelMocks.openDiscovery.mockReset();
    localStorage.removeItem("cc-switch-last-view");
  });

  it("covers basic provider flows via real hooks", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByText("usage"));
    expect(screen.getByTestId("usage-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByText("save-script"));
    fireEvent.click(screen.getByText("close-usage"));

    fireEvent.click(screen.getByText("create"));
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-add"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /New codex Provider/,
      ),
    );

    fireEvent.click(screen.getByText("edit"));
    expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-edit"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /-edited/,
      ),
    );

    fireEvent.click(screen.getByText("switch"));
    fireEvent.click(screen.getByText("duplicate"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(/copy/),
    );

    fireEvent.click(screen.getByText("open-website"));

    emitTauriEvent("provider-switched", {
      appType: "codex",
      providerId: "codex-2",
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).toHaveBeenCalled();
  }, 30_000);

  it("shows toast when auto sync fails in background", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });

    toastErrorMock.mockReset();
    expect(() => {
      emitTauriEvent("s3-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  }, 30_000);

  it("serializes startup attention so every visible recovery and routing action stays clickable", async () => {
    setSettings({ firstRunNoticeConfirmed: false });
    server.use(
      http.post("http://tauri.local/get_pending_recovery_outcomes", () =>
        HttpResponse.json([
          {
            id: "unclean-exit-1",
            generation: 7,
            operation: "startup",
            kind: "uncleanExit",
            severity: "warning",
            appType: "codex",
            keptFields: [],
            lostFields: [],
            nextStep: "reviewRecoveryResults",
            timestamp: "2026-08-31T10:00:00Z",
          },
        ]),
      ),
      http.post("http://tauri.local/acknowledge_recovery_outcomes", () =>
        HttpResponse.json(true),
      ),
      http.post("http://tauri.local/get_proxy_status", () =>
        HttpResponse.json({
          running: true,
          address: "127.0.0.1",
          port: 17801,
          active_connections: 0,
          total_requests: 0,
          success_requests: 0,
          failed_requests: 0,
          success_rate: 0,
          uptime_seconds: 1,
          current_provider: "codex-router",
          current_provider_id: "codex-router",
          last_request_at: null,
          last_error: null,
          failover_count: 0,
          active_targets: [],
        }),
      ),
      http.post("http://tauri.local/get_proxy_takeover_status", () =>
        HttpResponse.json({
          claude: false,
          codex: true,
          gemini: false,
          grokbuild: false,
        }),
      ),
      http.post("http://tauri.local/inspect_codex_config_consistency", () =>
        HttpResponse.json({
          state: "external_drift",
          providerId: "codex-router",
          expectedFingerprint: "expected",
          actualFingerprint: "actual",
          changedKeys: ["model"],
          reason: null,
        }),
      ),
      http.post("http://tauri.local/resolve_codex_config_consistency", () =>
        HttpResponse.json({
          state: "consistent",
          providerId: "codex-router",
          expectedFingerprint: "expected",
          actualFingerprint: "actual",
          changedKeys: [],
          reason: null,
        }),
      ),
    );

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() => expect(toastWarningMock).toHaveBeenCalledTimes(1));
    expect(screen.queryByText("codexConfigConsistency.title")).toBeNull();
    expect(screen.queryByText("firstRunNotice.title")).toBeNull();
    expect(screen.queryByText("Codex 本地路由已开启")).toBeNull();
    expect(document.body.style.pointerEvents).not.toBe("none");

    const recoveryOptions = toastWarningMock.mock.calls[0]?.[1] as
      | { onDismiss?: () => void }
      | undefined;
    expect(recoveryOptions?.onDismiss).toEqual(expect.any(Function));
    act(() => recoveryOptions?.onDismiss?.());

    expect(
      await screen.findByText("codexConfigConsistency.title"),
    ).toBeInTheDocument();
    expect(screen.queryByText("firstRunNotice.title")).toBeNull();
    expect(screen.queryByText("Codex 本地路由已开启")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "codexConfigConsistency.later" }),
    );

    expect(await screen.findByText("firstRunNotice.title")).toBeInTheDocument();
    expect(screen.queryByText("Codex 本地路由已开启")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "firstRunNotice.confirm" }),
    );

    expect(await screen.findByText("Codex 本地路由已开启")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "我知道了" }));
    await waitFor(() =>
      expect(screen.queryByText("Codex 本地路由已开启")).toBeNull(),
    );
  }, 30_000);

  it("offers safe port release from the blocking recovery toast", async () => {
    let forcedAppType: string | undefined;
    server.use(
      http.post("http://tauri.local/get_pending_recovery_outcomes", () =>
        HttpResponse.json([
          {
            id: "port-owner-1",
            generation: 8,
            operation: "proxy_listener_ownership",
            kind: "portOwnedByUnknownOwner",
            severity: "error",
            appType: "codex",
            keptFields: [],
            lostFields: [],
            nextStep: "changeProxyPortOrInspectOwner",
            timestamp: "2026-09-01T10:00:00Z",
          },
        ]),
      ),
      http.post(
        "http://tauri.local/force_release_proxy_port_and_restore_takeover",
        async ({ request }) => {
          const body = (await request.json()) as { appType?: string };
          forcedAppType = body.appType;
          return HttpResponse.json({
            appType: "codex",
            port: 15721,
            releasedPid: 41372,
            takeoverRestored: true,
          });
        },
      ),
      http.post("http://tauri.local/acknowledge_recovery_outcomes", () =>
        HttpResponse.json(true),
      ),
    );

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    const recoveryOptions = toastErrorMock.mock.calls[0]?.[1] as
      | { action?: { label?: string; onClick?: () => void } }
      | undefined;
    expect(recoveryOptions?.action?.label).toBe(
      "notifications.recovery.forceRestoreTakeover",
    );
    act(() => recoveryOptions?.action?.onClick?.());

    await waitFor(() => expect(forcedAppType).toBe("codex"));
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "notifications.recovery.forceRestoreSucceeded",
      ),
    );
  }, 30_000);

  it("duplicates openclaw providers with a generated key that avoids live-only ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");
    setLiveProviderIds("openclaw", ["deepseek-copy"]);

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      const providerList = screen.getByTestId("provider-list").textContent;
      expect(providerList).toContain("deepseek-copy-2");
      expect(providerList).toContain("DeepSeek copy");
    });

    expect(toastErrorMock).not.toHaveBeenCalledWith(
      expect.stringContaining("Provider key is required for openclaw"),
    );
  });

  it("opens Codex MultiRouter provider edits in the router workspace", async () => {
    setProviders("codex", {
      "codex-router": {
        id: "codex-router",
        name: "OpenAI Multi-Model Router",
        settingsConfig: {
          codexRouting: {
            enabled: true,
            defaultRouteId: "openai-official",
            routes: [
              {
                id: "openai-official",
                enabled: true,
                targetProviderId: "codex-openai",
                match: { prefixes: ["gpt-"] },
              },
            ],
          },
        },
        category: "aggregator",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("codex", "codex-router");

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-router",
      ),
    );

    fireEvent.click(screen.getByText("edit"));

    await waitFor(() =>
      expect(screen.getByTestId("codex-router-workspace")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("edit-provider-dialog")).toBeNull();
    expect(screen.getByTestId("codex-router-target").textContent).toBe(
      "codex-router",
    );
    expect(screen.getByTestId("codex-router-tab").textContent).toBe("routes");
  });

  it("opens explicit migration instead of directly enabling a schema v1 MultiRouter", async () => {
    setProviders("codex", {
      "legacy-codex-router": {
        id: "legacy-codex-router",
        name: "Legacy MultiRouter",
        settingsConfig: {
          codexRouting: {
            enabled: true,
            routes: [
              {
                id: "legacy-route",
                upstream: { apiFormat: "openai_chat", apiKey: "secret" },
              },
            ],
          },
        },
        category: "aggregator",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("codex", "legacy-codex-router");

    const { default: App } = await import("@/App");
    renderApp(App);
    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "legacy-codex-router",
      ),
    );

    fireEvent.click(screen.getByText("switch"));

    const wizard = await screen.findByTestId("codex-multirouter-wizard");
    expect(wizard).toHaveAttribute("data-mode", "edit");
    expect(wizard).toHaveAttribute("data-plan-id", "legacy-codex-router");
  }, 30_000);

  it("starts a new Codex MultiRouter draft explicitly from the refreshed entry", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByText("open-multirouter-entry"));

    expect(await screen.findByText("配置多路模型")).toBeInTheDocument();
    expect(screen.getByText("创建新配置")).toBeInTheDocument();
    expect(screen.getByText("编辑旧配置")).toBeInTheDocument();
    expect(
      screen.getByText("暂无已有 MultiRouter，请先创建新配置。"),
    ).toBeInTheDocument();
    expect(screen.getByText("直接打开工作台")).toBeInTheDocument();

    fireEvent.click(screen.getByText("创建新配置"));

    const wizard = await screen.findByTestId("codex-multirouter-wizard");
    expect(wizard).toHaveAttribute("data-mode", "create");
    expect(wizard).toHaveAttribute("data-plan-id", "");
  });

  it("returns a provider created from the wizard to the same draft by its new id", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
    fireEvent.click(screen.getByText("open-multirouter-entry"));
    fireEvent.click(await screen.findByText("创建新配置"));
    fireEvent.click(await screen.findByText("add-provider"));
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();

    setProviders("codex", {
      "wizard-new-source": {
        id: "wizard-new-source",
        name: "Wizard New Source",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          modelCatalog: { models: [{ model: "wizard-model" }] },
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    fireEvent.click(screen.getByText("close-add"));

    await waitFor(() =>
      expect(screen.getByTestId("codex-multirouter-wizard")).toHaveAttribute(
        "data-new-provider-ids",
        "wizard-new-source",
      ),
    );
  }, 30_000);

  it("routes the wizard history-repair action to the Codex session manager exactly once", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
    fireEvent.click(screen.getByText("open-multirouter-entry"));
    fireEvent.click(await screen.findByText("创建新配置"));
    fireEvent.click(await screen.findByText("open-history-repair"));

    const sessionManager = await screen.findByTestId("session-manager-page");
    expect(sessionManager).toHaveAttribute("data-app-id", "codex");
    expect(sessionManager).toHaveAttribute(
      "data-initial-history-repair",
      "true",
    );
    expect(screen.queryByTestId("codex-multirouter-wizard")).toBeNull();

    fireEvent.click(screen.getByText("consume-history-repair"));
    await waitFor(() =>
      expect(screen.getByTestId("session-manager-page")).toHaveAttribute(
        "data-initial-history-repair",
        "false",
      ),
    );
  });

  it("edits the exact MultiRouter selected from multiple saved plans", async () => {
    setProviders("codex", {
      "router-primary": {
        id: "router-primary",
        name: "主路由",
        settingsConfig: { codexRouting: { enabled: true, routes: [] } },
        category: "aggregator",
        sortIndex: 0,
        createdAt: Date.now(),
      },
      "router-lab": {
        id: "router-lab",
        name: "实验路由",
        settingsConfig: { codexRouting: { enabled: true, routes: [] } },
        category: "aggregator",
        sortIndex: 1,
        createdAt: Date.now() + 1,
      },
    });
    setCurrentProviderId("codex", "router-primary");

    const { default: App } = await import("@/App");
    renderApp(App);
    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "router-lab",
      ),
    );

    fireEvent.click(screen.getByText("open-multirouter-entry"));
    expect(await screen.findByText("主路由")).toBeInTheDocument();
    expect(screen.getByText("实验路由")).toBeInTheDocument();
    fireEvent.click(screen.getByText("实验路由"));

    const wizard = await screen.findByTestId("codex-multirouter-wizard");
    expect(wizard).toHaveAttribute("data-mode", "edit");
    expect(wizard).toHaveAttribute("data-plan-id", "router-lab");
  });

  it("opens the MultiRouter status workspace directly from the entry choice", async () => {
    setProviders("codex", {
      "codex-router": {
        id: "codex-router",
        name: "OpenAI Multi-Model Router",
        settingsConfig: {
          codexRouting: {
            enabled: true,
            routes: [],
          },
        },
        category: "aggregator",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("codex", "codex-router");

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-router",
      ),
    );

    fireEvent.click(screen.getByText("open-multirouter-entry"));
    fireEvent.click(await screen.findByText("直接打开工作台"));

    await waitFor(() =>
      expect(screen.getByTestId("codex-router-workspace")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("codex-router-target").textContent).toBe(
      "codex-router",
    );
    expect(screen.getByTestId("codex-router-tab").textContent).toBe("status");
    expect(screen.getByTestId("codex-router-workspace")).toHaveAttribute(
      "data-runtime-ready-wired",
      "false",
    );
  });

  it("opens the Codex usage page from the Codex toolbar", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByTitle("Codex 用量与重置额度"));

    await waitFor(() =>
      expect(screen.getByTestId("codex-usage-page")).toBeInTheDocument(),
    );
  });

  it("opens the persistent Codex status and repair entry when no drift dialog exists", async () => {
    server.use(
      http.post("http://tauri.local/inspect_codex_config_consistency", () =>
        HttpResponse.json({
          state: "consistent",
          providerId: "codex-1",
          expectedFingerprint: "expected",
          actualFingerprint: "actual",
          changedKeys: [],
          reason: null,
          runtimeActivation: {
            state: "current",
            appServerStartedAt: "2026-09-02T01:00:00+08:00",
            configModifiedAt: "2026-09-02T00:59:00+08:00",
            reason: null,
          },
        }),
      ),
      http.post("http://tauri.local/inspect_codex_runtime_refresh", () =>
        HttpResponse.json({
          supported: true,
          canRefresh: true,
          snapshotToken: "status-snapshot",
          desktopProcessCount: 1,
          appServerProcessCount: 1,
          processCount: 2,
          launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
          warning: null,
          paginatedHistory: {
            affectedRolloutCount: 0,
            duplicateOrdinalCount: 0,
            affectedBytes: 0,
            blockedRolloutCount: 0,
            blockedReason: null,
          },
        }),
      ),
    );
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
    fireEvent.click(screen.getByTitle("codexConfigConsistency.statusTitle"));

    expect(
      await screen.findByRole("dialog", {
        name: "codexConfigConsistency.statusTitle",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("codexConfigConsistency.configStatus"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("codexConfigConsistency.paginatedHistoryStatus"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("codexConfigConsistency.rendererCompatibilityStatus"),
    ).toBeInTheDocument();
  });

  it("shows toast when duplicate cannot load live provider ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");

    const liveIdsSpy = vi
      .spyOn(providersApi, "getOpenClawLiveProviderIds")
      .mockRejectedValueOnce(new Error("broken config"));

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("读取配置中的供应商标识失败"),
      );
    });

    expect(screen.getByTestId("provider-list").textContent).not.toContain(
      "deepseek-copy",
    );

    liveIdsSpy.mockRestore();
  });

  it("hosts the Skills check-update action in the App toolbar", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    const checkUpdatesButton = await screen.findByRole("button", {
      name: "skills.checkUpdates",
    });
    await waitFor(() => expect(checkUpdatesButton).toBeEnabled());

    fireEvent.click(checkUpdatesButton);
    expect(skillsPanelMocks.checkUpdates).toHaveBeenCalledTimes(1);
  });

  it("routes the Skills discover toolbar action through the panel guard", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "skills.discover",
      }),
    );

    expect(skillsPanelMocks.openDiscovery).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("unified-skills-panel")).toBeInTheDocument();
  });
});
