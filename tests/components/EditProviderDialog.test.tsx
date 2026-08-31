import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  getCurrent: vi.fn(),
  getCodexLogicalProviderForEditing: vi.fn(),
  getLiveProviderSettings: vi.fn(),
  getOpenClawLiveProvider: vi.fn(),
  updateTrayMenu: vi.fn(),
  prepareCodexProviderSet: vi.fn(),
  commitCodexProviderSet: vi.fn(),
  preflightCodexProviderProtocolCompatibility: vi.fn(),
  getCodexProviderEditorSnapshot: vi.fn(),
  invalidateQueries: vi.fn(),
}));
const formSubmission = vi.hoisted(() => ({
  receiptIds: [] as string[],
}));

vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return {
    ...actual,
    useQueryClient: () => ({ invalidateQueries: apiMocks.invalidateQueries }),
  };
});

vi.mock("@/lib/api", () => ({
  providersApi: {
    getCurrent: apiMocks.getCurrent,
    getCodexLogicalProviderForEditing:
      apiMocks.getCodexLogicalProviderForEditing,
    updateTrayMenu: apiMocks.updateTrayMenu,
  },
  vscodeApi: {
    getLiveProviderSettings: apiMocks.getLiveProviderSettings,
  },
  openclawApi: {
    getLiveProvider: apiMocks.getOpenClawLiveProvider,
  },
}));

vi.mock("@/lib/api/protocol-compatibility", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/api/protocol-compatibility")>();
  return {
    ...actual,
    prepareCodexProviderSet: apiMocks.prepareCodexProviderSet,
    commitCodexProviderSet: apiMocks.commitCodexProviderSet,
    preflightCodexProviderProtocolCompatibility:
      apiMocks.preflightCodexProviderProtocolCompatibility,
    getCodexProviderEditorSnapshot: apiMocks.getCodexProviderEditorSnapshot,
  };
});

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({
    initialData,
    codexProviderEditorSnapshot,
    onSubmit,
    isProxyTakeover,
  }: {
    initialData: {
      name?: string;
      websiteUrl?: string;
      notes?: string;
      settingsConfig?: Record<string, unknown>;
      meta?: Record<string, unknown>;
      icon?: string;
      iconColor?: string;
      protocolProbeReceiptIds?: string[];
    };
    codexProviderEditorSnapshot?: {
      adaptation?: { effectiveTransport?: string | null };
    } | null;
    onSubmit: (values: {
      name: string;
      websiteUrl: string;
      notes?: string;
      settingsConfig: string;
      meta?: Record<string, unknown>;
      icon?: string;
      iconColor?: string;
      protocolProbeReceiptIds?: string[];
    }) => void;
    isProxyTakeover?: boolean;
  }) => (
    <form
      id="provider-form"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit({
          name: initialData.name ?? "",
          websiteUrl: initialData.websiteUrl ?? "",
          notes: initialData.notes,
          settingsConfig: JSON.stringify(initialData.settingsConfig ?? {}),
          meta: initialData.meta,
          icon: initialData.icon,
          iconColor: initialData.iconColor,
          protocolProbeReceiptIds: formSubmission.receiptIds,
        });
      }}
    >
      <output data-testid="settings-config">
        {JSON.stringify(initialData.settingsConfig ?? {})}
      </output>
      <output data-testid="is-proxy-takeover">
        {isProxyTakeover ? "true" : "false"}
      </output>
      <output data-testid="codex-editor-snapshot">
        {JSON.stringify(codexProviderEditorSnapshot ?? null)}
      </output>
    </form>
  ),
}));

import { EditProviderDialog } from "@/components/providers/EditProviderDialog";

function editorSnapshotFor(provider: Provider) {
  return {
    logicalProvider: provider,
    adaptation: {
      persistence: "single",
      status: "not_tested",
      effectiveTransport: null,
      models: [],
    },
  };
}

describe("EditProviderDialog", () => {
  beforeEach(() => {
    apiMocks.getCurrent.mockReset();
    apiMocks.getCodexLogicalProviderForEditing.mockReset();
    apiMocks.getLiveProviderSettings.mockReset();
    apiMocks.getOpenClawLiveProvider.mockReset();
    apiMocks.updateTrayMenu.mockReset().mockResolvedValue(undefined);
    apiMocks.prepareCodexProviderSet.mockReset().mockResolvedValue({
      digest: "edit-digest",
      sourceProviderId: "deepseek",
      responsesModels: ["deepseek-v4"],
      chatModels: [],
      plan: { kind: "single", transport: "open_ai_responses" },
    });
    apiMocks.commitCodexProviderSet.mockReset().mockResolvedValue({
      preview: {
        digest: "edit-digest",
        sourceProviderId: "deepseek",
        responsesModels: ["deepseek-v4"],
        chatModels: [],
        plan: { kind: "single", transport: "open_ai_responses" },
      },
      projections: [],
      status: "committed",
    });
    apiMocks.preflightCodexProviderProtocolCompatibility.mockReset();
    apiMocks.getCodexProviderEditorSnapshot.mockReset();
    apiMocks.invalidateQueries.mockReset().mockResolvedValue(undefined);
    formSubmission.receiptIds = [];
  });

  it("loads the authoritative editor snapshot for an ordinary Codex provider", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
        config: 'model = "deepseek-v4"',
        modelCatalog: { models: [{ model: "deepseek-v4" }] },
      },
    };
    const logicalProvider: Provider = {
      ...provider,
      meta: {
        codexProtocolMode: "manual",
        codexProtocolOverrides: { "deepseek-v4": "openai_chat" },
      },
    };
    let resolveSnapshot!: (value: unknown) => void;
    apiMocks.getCodexProviderEditorSnapshot.mockReturnValue(
      new Promise((resolve) => {
        resolveSnapshot = resolve;
      }),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );

    await waitFor(() =>
      expect(apiMocks.getCodexProviderEditorSnapshot).toHaveBeenCalledWith(
        "deepseek",
      ),
    );
    expect(screen.queryByTestId("settings-config")).not.toBeInTheDocument();

    resolveSnapshot({
      logicalProvider,
      adaptation: {
        persistence: "single",
        status: "ready",
        effectiveTransport: "open_ai_chat",
        testedAt: 1_777_777_777,
        expiresAt: 1_888_888_888,
        models: [],
      },
    });

    await screen.findByTestId("settings-config");
    expect(
      JSON.parse(screen.getByTestId("codex-editor-snapshot").textContent ?? ""),
    ).toMatchObject({
      adaptation: {
        persistence: "single",
        status: "ready",
        effectiveTransport: "open_ai_chat",
      },
    });
  });

  it("保留 Codex 数据库中的 modelCatalog，避免 live 配置缺字段时清空模型映射", async () => {
    const dbModelCatalog = {
      models: [
        {
          model: "deepseek-v4-flash",
          displayName: "DeepSeek V4 Flash",
          contextWindow: 1000000,
        },
      ],
    };
    const dbCodexRouting = {
      enabled: true,
      defaultRouteId: "openai-official",
      routes: [
        {
          id: "deepseek",
          enabled: true,
          match: { models: ["deepseek-v4-flash"], prefixes: [] },
          upstream: {
            baseUrl: "https://api.deepseek.com",
            apiFormat: "openai_chat",
            auth: { source: "provider_config" },
          },
        },
      ],
    };
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "db-key",
        },
        config: 'model_provider = "custom"\nmodel = "deepseek-v4-flash"\n',
        modelCatalog: dbModelCatalog,
        codexRouting: dbCodexRouting,
      },
    };
    const liveSettings = {
      auth: {
        OPENAI_API_KEY: "live-key",
      },
      config: 'model_provider = "custom"\nmodel = "deepseek-v4-pro"\n',
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(liveSettings);
    apiMocks.getCodexProviderEditorSnapshot.mockResolvedValue(
      editorSnapshotFor(provider),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual({
        ...liveSettings,
        modelCatalog: dbModelCatalog,
        codexRouting: dbCodexRouting,
      });
    });

    fireEvent.click(await screen.findByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].provider.settingsConfig).toEqual({
      ...liveSettings,
      modelCatalog: dbModelCatalog,
      codexRouting: dbCodexRouting,
    });
  });

  it("代理接管中编辑 Codex 供应商时展示数据库配置而不是读取 live 代理配置", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "db-key",
        },
        config:
          'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://api.deepseek.com/v1"\n',
      },
    };

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue({
      auth: {
        OPENAI_API_KEY: "PROXY_MANAGED",
      },
      config:
        'model_provider = "custom"\n[model_providers.custom]\nbase_url = "http://127.0.0.1:15721/v1"\nexperimental_bearer_token = "PROXY_MANAGED"\n',
    });
    apiMocks.getCodexProviderEditorSnapshot.mockResolvedValue(
      editorSnapshotFor(provider),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("is-proxy-takeover").textContent).toBe("true");
    });

    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);
  });

  it("普通 Codex 编辑消费现有 receipt 并提交 Provider Set，而不调用旧更新接口", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
        config:
          'model_provider = "deepseek"\nmodel = "deepseek-v4"\n[model_providers.deepseek]\nbase_url = "https://api.deepseek.com/v1"\nwire_api = "responses"\n',
        modelCatalog: { models: [{ model: "deepseek-v4" }] },
      },
      meta: { apiFormat: "openai_responses" },
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);
    const handleOpenChange = vi.fn();
    formSubmission.receiptIds = ["receipt-deepseek-v4"];
    apiMocks.getCodexProviderEditorSnapshot.mockResolvedValue(
      editorSnapshotFor(provider),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={handleOpenChange}
        onSubmit={handleSubmit}
        appId="codex"
        isProxyTakeover
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(apiMocks.commitCodexProviderSet).toHaveBeenCalledWith(
        expect.objectContaining({ id: "deepseek", name: "DeepSeek" }),
        ["receipt-deepseek-v4"],
        "edit-digest",
        "accept_auto",
      ),
    );
    expect(
      apiMocks.preflightCodexProviderProtocolCompatibility,
    ).not.toHaveBeenCalled();
    expect(handleSubmit).not.toHaveBeenCalled();
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });

  it("Codex 编辑手动 Chat 覆盖保留 receipt 并以 confirm_manual 提交", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
        config:
          'model_provider = "deepseek"\nmodel = "deepseek-v4"\n[model_providers.deepseek]\nbase_url = "https://api.deepseek.com/v1"\nwire_api = "chat"\n',
        modelCatalog: { models: [{ model: "deepseek-v4" }] },
      },
      meta: {
        apiFormat: "openai_chat",
        codexProtocolMode: "manual",
      },
    };
    formSubmission.receiptIds = ["receipt-deepseek-v4"];
    apiMocks.getCodexProviderEditorSnapshot.mockResolvedValue(
      editorSnapshotFor(provider),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(apiMocks.commitCodexProviderSet).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "deepseek",
          meta: expect.objectContaining({
            apiFormat: "openai_chat",
            codexProtocolMode: "manual",
          }),
        }),
        ["receipt-deepseek-v4"],
        "edit-digest",
        "confirm_manual",
      ),
    );
    expect(
      apiMocks.preflightCodexProviderProtocolCompatibility,
    ).not.toHaveBeenCalled();
  });

  it("编辑自动拆分门面时从 editor snapshot 恢复逻辑 Provider 和拆分证据", async () => {
    const facade: Provider = {
      id: "qwen",
      name: "Qwen",
      category: "custom",
      settingsConfig: {
        codexProtocolSet: {
          version: 1,
          role: "facade",
          responsesProviderId: "qwen--ccsm-responses",
          chatProviderId: "qwen--ccsm-chat",
        },
        codexRouting: {
          schemaVersion: 2,
          routes: [
            { targetProviderId: "qwen--ccsm-responses" },
            { targetProviderId: "qwen--ccsm-chat" },
          ],
        },
      },
      meta: { apiFormat: "openai_responses" },
    };
    const logical: Provider = {
      ...facade,
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
        config:
          'model_provider = "qwen"\nmodel = "qwen3.8"\n[model_providers.qwen]\nbase_url = "https://qwen.example/v1"\nwire_api = "responses"\n',
        modelCatalog: {
          models: [{ model: "qwen3.8" }, { model: "qwen-coder" }],
        },
      },
    };
    apiMocks.getCodexProviderEditorSnapshot.mockResolvedValue({
      logicalProvider: logical,
      adaptation: {
        persistence: "split",
        status: "ready",
        effectiveTransport: "mixed",
        models: [],
      },
    });

    render(
      <EditProviderDialog
        open
        provider={facade}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );

    await waitFor(() =>
      expect(apiMocks.getCodexProviderEditorSnapshot).toHaveBeenCalledWith(
        "qwen",
      ),
    );
    expect(apiMocks.getCodexLogicalProviderForEditing).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(logical.settingsConfig),
    );
    expect(
      JSON.parse(screen.getByTestId("codex-editor-snapshot").textContent ?? ""),
    ).toMatchObject({ adaptation: { persistence: "split" } });
  });
});
