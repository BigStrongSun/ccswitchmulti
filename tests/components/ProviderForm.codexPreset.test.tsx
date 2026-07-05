import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

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
    takeoverEnabled,
    onCatalogModelsChange,
  }: {
    codexApiKey: string;
    codexBaseUrl: string;
    catalogModels?: Array<{ model: string; contextWindow?: number | string }>;
    takeoverEnabled: boolean;
    onCatalogModelsChange?: (
      models: Array<{ model: string; contextWindow?: number | string }>,
    ) => void;
  }) => (
    <section aria-label="codex-provider-details">
      <div data-testid="codex-api-key">{codexApiKey}</div>
      <div data-testid="codex-base-url">{codexBaseUrl}</div>
      <div data-testid="codex-takeover">
        {takeoverEnabled ? "enabled" : "disabled"}
      </div>
      <div data-testid="codex-catalog">
        {(catalogModels ?? []).map((model) => model.model).join(",")}
      </div>
      <button
        type="button"
        onClick={() =>
          onCatalogModelsChange?.([{ model: "gpt-5.5", contextWindow: 272000 }])
        }
      >
        mock-set-catalog
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
});
