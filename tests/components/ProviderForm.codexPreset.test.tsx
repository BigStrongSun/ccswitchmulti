import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

const codexCandidateApiMocks = vi.hoisted(() => ({
  validateProviderCandidate: vi.fn(),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: null }),
}));

vi.mock("@/hooks/useCopilotAuth", () => ({
  useCopilotAuth: () => ({ isAuthenticated: false }),
}));

vi.mock("@/hooks/useOpenClaw", () => ({
  useOpenClawLiveProviderIds: () => ({ data: [], isLoading: false }),
}));

vi.mock("@/hooks/useHermes", () => ({
  useHermesLiveProviderIds: () => ({ data: [], isLoading: false }),
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    authApi: {
      authGetStatus: vi.fn().mockResolvedValue({ authenticated: false }),
      authStartLogin: vi.fn(),
      authPollForAccount: vi.fn(),
      authLogout: vi.fn(),
      authRemoveAccount: vi.fn(),
      authSetDefaultAccount: vi.fn(),
    },
    configApi: {
      getCommonConfigSnippet: vi.fn().mockResolvedValue(null),
      saveCommonConfigSnippet: vi.fn(),
      deleteCommonConfigSnippet: vi.fn(),
    },
    codexSubagentV2Api: {
      ...actual.codexSubagentV2Api,
      validateProviderCandidate: (...args: unknown[]) =>
        codexCandidateApiMocks.validateProviderCandidate(...args),
    },
  };
});

vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => (
    <section aria-label="provider-advanced-config" />
  ),
}));

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: ({
    authValue,
    configValue,
  }: {
    authValue: string;
    configValue: string;
  }) => (
    <section aria-label="codex-config-editor">
      <pre data-testid="codex-auth-editor">{authValue}</pre>
      <pre data-testid="codex-config-editor">{configValue}</pre>
    </section>
  ),
}));

vi.mock("@/components/providers/forms/CodexFormFields", () => ({
  CodexFormFields: ({
    codexApiKey,
    codexBaseUrl,
    catalogModels,
    presetCatalogModels,
    takeoverEnabled,
    allowModelMenuProjectionToggle,
    onApiKeyChange,
    onCatalogModelsChange,
    onProtocolProbeReceiptIdsChange,
    onProtocolModeChange,
    onProtocolOverridesChange,
    onApiFormatChange,
  }: {
    codexApiKey: string;
    codexBaseUrl: string;
    catalogModels?: Array<{
      model: string;
      contextWindow?: number | string;
      reasoning?: unknown;
    }>;
    presetCatalogModels?: Array<{ model: string; reasoning?: unknown }>;
    takeoverEnabled: boolean;
    allowModelMenuProjectionToggle?: boolean;
    onApiKeyChange?: (value: string) => void;
    onCatalogModelsChange?: (
      models: Array<{ model: string; contextWindow?: number | string }>,
    ) => void;
    onProtocolProbeReceiptIdsChange?: (receiptIds: string[]) => void;
    onProtocolModeChange?: (mode: "auto" | "manual") => void;
    onProtocolOverridesChange?: (
      overrides: Record<string, "openai_chat" | "openai_responses">,
    ) => void;
    onApiFormatChange?: (format: "openai_chat" | "openai_responses") => void;
  }) => (
    <section aria-label="codex-provider-details">
      <div data-testid="codex-api-key">{codexApiKey}</div>
      <div data-testid="codex-base-url">{codexBaseUrl}</div>
      <div data-testid="codex-takeover">
        {takeoverEnabled ? "enabled" : "disabled"}
      </div>
      <div data-testid="codex-menu-projection-editability">
        {allowModelMenuProjectionToggle ? "editable" : "managed"}
      </div>
      <div data-testid="codex-catalog">
        {(catalogModels ?? []).map((model) => model.model).join(",")}
      </div>
      <div data-testid="codex-preset-reasoning-models">
        {(presetCatalogModels ?? [])
          .filter((model) => model.reasoning)
          .map((model) => model.model)
          .join(",")}
      </div>
      <button
        type="button"
        onClick={() =>
          onCatalogModelsChange?.([{ model: "gpt-5.5", contextWindow: 272000 }])
        }
      >
        mock-set-catalog
      </button>
      <button type="button" onClick={() => onApiKeyChange?.("sk-test")}>
        mock-set-api-key
      </button>
      <button
        type="button"
        onClick={() => onProtocolProbeReceiptIdsChange?.(["receipt-model-a"])}
      >
        mock-set-probe-receipts
      </button>
      <button
        type="button"
        onClick={() => {
          onProtocolModeChange?.("manual");
          onApiFormatChange?.("openai_chat");
        }}
      >
        mock-set-manual-chat
      </button>
      <button
        type="button"
        onClick={() => {
          onProtocolModeChange?.("auto");
          onProtocolOverridesChange?.({});
        }}
      >
        mock-set-auto-protocol
      </button>
      <button
        type="button"
        onClick={() =>
          onProtocolOverridesChange?.({ "model-a": "openai_chat" })
        }
      >
        mock-set-protocol-override
      </button>
    </section>
  ),
  buildSplitCodexProviderSuggestionForFetchedModels: vi.fn(),
}));

function renderProviderForm(
  props: Partial<ComponentProps<typeof ProviderForm>> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId="codex"
        submitLabel="添加"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
        showButtons={false}
        {...props}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderForm Codex preset selection", () => {
  beforeEach(() => {
    codexCandidateApiMocks.validateProviderCandidate
      .mockReset()
      .mockResolvedValue(undefined);
  });

  it("defaults new Codex providers to model menu projection", async () => {
    renderProviderForm();

    await waitFor(() => {
      expect(screen.getByTestId("codex-takeover")).toHaveTextContent("enabled");
    });
  });

  it("forces a saved maintained preset back to model menu projection", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Zhipu legacy opt-out",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "zhipu"\nmodel = "glm-5.2"\n[model_providers.zhipu]\nbase_url = "https://open.bigmodel.cn/api/coding/paas/v4"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "glm-5.2" }] },
        },
        meta: {
          apiFormat: "openai_responses",
          codexLocalModelMapping: false,
          codexPresetId: "zhipu-glm-cn",
        },
      },
    });

    await waitFor(() => {
      expect(screen.getByTestId("codex-takeover")).toHaveTextContent("enabled");
      expect(
        screen.getByTestId("codex-menu-projection-editability"),
      ).toHaveTextContent("managed");
    });
    fireEvent.click(screen.getByRole("button", { name: "mock-set-api-key" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(onSubmit.mock.calls[0][0].meta.codexLocalModelMapping).toBe(true);
  });

  it("does not scroll when applying the default Codex source preset on mount", async () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });

    renderProviderForm();

    await waitFor(() => {
      expect(screen.getByTestId("codex-api-key")).toBeInTheDocument();
    });
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("passes deep-probe receipt leases to the outer save coordinator without persisting them in settings", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Third-party relay",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "relay"\nmodel = "model-a"\n[model_providers.relay]\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "model-a" }] },
        },
        meta: { apiFormat: "openai_responses" },
      },
    });

    fireEvent.click(
      await screen.findByRole("button", { name: "mock-set-probe-receipts" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].protocolProbeReceiptIds).toEqual([
      "receipt-model-a",
    ]);
    expect(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig),
    ).not.toHaveProperty("protocolProbeReceiptIds");
  });

  it("keeps deep-probe receipts when advanced mode selects Chat as the final protocol", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Third-party relay",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "relay"\nmodel = "model-a"\n[model_providers.relay]\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "model-a" }] },
        },
        meta: { apiFormat: "openai_responses" },
      },
    });

    fireEvent.click(
      await screen.findByRole("button", { name: "mock-set-probe-receipts" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "mock-set-manual-chat" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].protocolProbeReceiptIds).toEqual([
      "receipt-model-a",
    ]);
    expect(onSubmit.mock.calls[0][0].meta).toEqual(
      expect.objectContaining({
        codexProtocolMode: "manual",
        apiFormat: "openai_chat",
      }),
    );
  });

  it("clears legacy manual intent when the user returns a Provider to automatic protocol selection", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Legacy Qwen relay",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "qwen"\nmodel = "qwen3.8"\n[model_providers.qwen]\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "qwen3.8" }] },
        },
        meta: {
          apiFormat: "openai_responses",
          codexProtocolMode: "manual",
          codexProtocolOverrides: { "qwen3.8": "openai_responses" },
        },
      },
    });

    fireEvent.click(
      await screen.findByRole("button", { name: "mock-set-probe-receipts" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "mock-set-auto-protocol" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].protocolProbeReceiptIds).toEqual([
      "receipt-model-a",
    ]);
    expect(onSubmit.mock.calls[0][0].meta).not.toHaveProperty(
      "codexProtocolMode",
    );
    expect(onSubmit.mock.calls[0][0].meta).not.toHaveProperty(
      "codexProtocolOverrides",
    );
  });

  it("persists a per-model protocol override in Provider meta", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Third-party relay",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "relay"\nmodel = "model-a"\n[model_providers.relay]\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "model-a" }] },
        },
        meta: { apiFormat: "openai_responses" },
      },
    });

    fireEvent.click(
      await screen.findByRole("button", {
        name: "mock-set-protocol-override",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].meta).toEqual(
      expect.objectContaining({
        codexProtocolOverrides: { "model-a": "openai_chat" },
      }),
    );
    expect(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).modelCatalog
        .models[0],
    ).not.toHaveProperty("apiFormat");
  });

  it("scrolls to Codex provider details after selecting any Codex source preset", async () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });

    renderProviderForm();

    fireEvent.click(screen.getByRole("button", { name: /DeepSeek$/ }));

    await waitFor(() => {
      expect(screen.getByTestId("codex-base-url")).toHaveTextContent(
        "https://api.deepseek.com",
      );
      expect(
        screen.getByTestId("codex-menu-projection-editability"),
      ).toHaveTextContent("managed");
    });
    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "start",
    });

    scrollIntoView.mockClear();
    fireEvent.click(screen.getByRole("button", { name: /Zhipu GLM$/ }));

    await waitFor(() => {
      expect(screen.getByTestId("codex-base-url")).toHaveTextContent(
        "https://open.bigmodel.cn/api/coding/paas/v4",
      );
    });
    expect(screen.getByTestId("codex-catalog")).toHaveTextContent("glm-5.2");
    expect(screen.getByTestId("codex-takeover")).toHaveTextContent("enabled");
    await waitFor(() => {
      expect(scrollIntoView).toHaveBeenCalledWith({
        behavior: "smooth",
        block: "start",
      });
    });
  });

  it("persists catalog metadata without enabling Codex menu mapping", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Native Responses",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "native"\nmodel = "gpt-5.5"\n[model_providers.native]\nbase_url = "https://api.example.com/v1"\nwire_api = "responses"\n',
        },
        meta: {
          apiFormat: "openai_responses",
          codexLocalModelMapping: false,
        },
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "mock-set-catalog" }));
    await waitFor(() => {
      expect(screen.getByTestId("codex-catalog")).toHaveTextContent("gpt-5.5");
    });
    fireEvent.click(screen.getByRole("button", { name: "mock-set-api-key" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalled();
    });
    const payload = onSubmit.mock.calls[0][0];
    const savedSettings = JSON.parse(payload.settingsConfig);
    expect(payload.meta.codexLocalModelMapping).toBe(false);
    expect(savedSettings.modelCatalog.models).toEqual([
      { model: "gpt-5.5", contextWindow: 272000 },
    ]);
  });

  it("preserves the complete Sub-Agent V2 document during an ordinary provider save", async () => {
    const onSubmit = vi.fn();
    const subagentV2 = {
      schemaVersion: 2,
      selectionPolicy: "balanced",
      profiles: {
        "deepseek-v4-pro": {
          model: "deepseek-v4-pro",
          enabled: true,
          inputModalities: ["text"],
          questionnaire: {
            taskStrengths: ["complex_debugging"],
            optimization: "quality",
            writeScope: "complex_changes",
            preference: "preferred",
          },
          reasoning: { policy: "fixed", effort: "high" },
        },
      },
    };
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Existing MultiRouter",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "codex_model_router_v2"\nmodel = "deepseek-v4-pro"\n[model_providers.codex_model_router_v2]\nbase_url = "http://127.0.0.1:15721/v1"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "deepseek-v4-pro" }] },
          codexRouting: {
            enabled: true,
            defaultRouteId: "deepseek",
            subagentVersion: "v2",
            subagentV2,
            routes: [],
          },
        },
        meta: {
          apiFormat: "openai_responses",
          codexLocalModelMapping: true,
        },
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const savedSettings = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(savedSettings.codexRouting.subagentVersion).toBe("v2");
    expect(savedSettings.codexRouting.subagentV2).toEqual(subagentV2);
  });

  it("blocks an ordinary provider save before persistence when reasoning is unknown", async () => {
    const onSubmit = vi.fn();
    codexCandidateApiMocks.validateProviderCandidate.mockRejectedValueOnce(
      new Error("unknown_reasoning_capability_requires_declaration"),
    );
    renderProviderForm({
      showButtons: true,
      submitLabel: "保存",
      onSubmit,
      initialData: {
        name: "Incomplete MultiRouter",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "codex_model_router_v2"\nmodel = "unknown-model"\n[model_providers.codex_model_router_v2]\nbase_url = "http://127.0.0.1:15721/v1"\nwire_api = "responses"\n',
          modelCatalog: { models: [{ model: "unknown-model" }] },
          codexRouting: {
            enabled: true,
            defaultRouteId: "unknown-route",
            subagentVersion: "v2",
            subagentV2: {
              schemaVersion: 2,
              selectionPolicy: "balanced",
              profiles: {
                "unknown-model": {
                  model: "unknown-model",
                  enabled: true,
                  questionnaire: {
                    taskStrengths: ["repository_exploration"],
                    optimization: "balanced",
                    writeScope: "read_only",
                    preference: "eligible",
                  },
                  reasoning: { policy: "delegated" },
                },
              },
            },
            routes: [],
          },
        },
        meta: {
          apiFormat: "openai_responses",
          codexLocalModelMapping: true,
        },
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(
        codexCandidateApiMocks.validateProviderCandidate,
      ).toHaveBeenCalledTimes(1);
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("persists maintained reasoning capabilities after selecting a built-in provider", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({ showButtons: true, submitLabel: "保存", onSubmit });

    fireEvent.click(screen.getByRole("button", { name: /Zhipu GLM$/ }));
    await waitFor(() => {
      expect(screen.getByTestId("codex-catalog")).toHaveTextContent("glm-5.2");
    });
    fireEvent.click(screen.getByRole("button", { name: "mock-set-api-key" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const savedSettings = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(onSubmit.mock.calls[0][0].meta.codexPresetId).toBe("zhipu-glm-cn");
    expect(
      screen.getByTestId("codex-preset-reasoning-models"),
    ).toHaveTextContent("glm-5.2");
    expect(savedSettings.modelCatalog.models).toHaveLength(1);
    for (const model of savedSettings.modelCatalog.models) {
      expect(model.reasoning).toMatchObject({
        supportedEfforts: [
          "none",
          "minimal",
          "low",
          "medium",
          "high",
          "xhigh",
          "max",
        ],
        defaultEffort: "max",
        source: "builtin",
      });
    }
  });

  it("restores the maintained preset baseline when reopening a saved override", async () => {
    renderProviderForm({
      initialData: {
        name: "Zhipu override",
        category: "custom",
        settingsConfig: {
          auth: { OPENAI_API_KEY: "sk-test" },
          config:
            'model_provider = "zhipu"\nmodel = "glm-5.2"\n[model_providers.zhipu]\nbase_url = "https://open.bigmodel.cn/api/coding/paas/v4"\nwire_api = "responses"\n',
          modelCatalog: {
            models: [
              {
                model: "glm-5.2",
                reasoning: {
                  supported: true,
                  supportedEfforts: ["low", "high"],
                  defaultEffort: "high",
                  disableAllowed: false,
                  upstream: {
                    format: "reasoning_object",
                    parameter: "reasoning.effort",
                  },
                  source: "user",
                },
              },
            ],
          },
        },
        meta: {
          apiFormat: "openai_responses",
          codexLocalModelMapping: true,
          codexPresetId: "zhipu-glm-cn",
        },
      },
    });

    await waitFor(() => {
      expect(
        screen.getByTestId("codex-preset-reasoning-models"),
      ).toHaveTextContent("glm-5.2");
    });
  });

  it("clears the maintained preset identity after switching to a custom source", async () => {
    const onSubmit = vi.fn();
    renderProviderForm({ showButtons: true, submitLabel: "保存", onSubmit });

    fireEvent.click(screen.getByRole("button", { name: /Zhipu GLM$/ }));
    await waitFor(() => {
      expect(screen.getByTestId("codex-catalog")).toHaveTextContent("glm-5.2");
    });
    fireEvent.click(screen.getByRole("button", { name: "自定义模型源" }));
    expect(screen.getByTestId("codex-takeover")).toHaveTextContent("enabled");
    expect(
      screen.getByTestId("codex-menu-projection-editability"),
    ).toHaveTextContent("editable");
    fireEvent.change(screen.getByRole("textbox", { name: "provider.name" }), {
      target: { value: "Custom source" },
    });
    fireEvent.click(screen.getByRole("button", { name: "mock-set-api-key" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    fireEvent.click(await screen.findByRole("button", { name: "仍要保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(onSubmit.mock.calls[0][0].meta.codexPresetId).toBeUndefined();
    expect(
      screen.getByTestId("codex-preset-reasoning-models"),
    ).toBeEmptyDOMElement();
  });
});
