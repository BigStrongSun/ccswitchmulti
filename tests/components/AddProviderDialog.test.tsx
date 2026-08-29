import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import type { ProviderFormValues } from "@/components/providers/forms/ProviderForm";
import type { UniversalProvider } from "@/types";

const universalApiMocks = vi.hoisted(() => ({
  saveAndSync: vi.fn(),
}));
const queryClientMocks = vi.hoisted(() => ({
  invalidateQueries: vi.fn(),
}));
const universalProtocolMocks = vi.hoisted(() => ({
  preflightCodex: vi.fn(),
  prepareCodex: vi.fn(),
  commitCodex: vi.fn(),
  preflight: vi.fn(),
  prepare: vi.fn(),
  commit: vi.fn(),
}));
const universalFormSubmission = vi.hoisted(() => ({
  current: null as UniversalProvider | null,
}));

vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return {
    ...actual,
    useQueryClient: () => ({
      invalidateQueries: queryClientMocks.invalidateQueries,
    }),
  };
});

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    universalProvidersApi: {
      ...actual.universalProvidersApi,
      saveAndSync: universalApiMocks.saveAndSync,
    },
  };
});
vi.mock("@/lib/api/protocol-compatibility", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/api/protocol-compatibility")>();
  return {
    ...actual,
    preflightCodexProviderProtocolCompatibility:
      universalProtocolMocks.preflightCodex,
    prepareCodexProviderSet: universalProtocolMocks.prepareCodex,
    commitCodexProviderSet: universalProtocolMocks.commitCodex,
    preflightUniversalCodexProtocolCompatibility:
      universalProtocolMocks.preflight,
    prepareUniversalProviderSet: universalProtocolMocks.prepare,
    commitUniversalProviderSet: universalProtocolMocks.commit,
  };
});
vi.mock("@/components/universal", () => ({
  UniversalProviderPanel: () => null,
}));
vi.mock("@/components/universal/UniversalProviderFormModal", () => ({
  UniversalProviderFormModal: ({
    isOpen,
    onSave,
  }: {
    isOpen: boolean;
    onSave: (provider: UniversalProvider) => void | Promise<void>;
  }) =>
    isOpen ? (
      <button
        type="button"
        onClick={() => void onSave(universalFormSubmission.current!)}
      >
        submit-universal-provider
      </button>
    ) : null,
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

let mockFormValues: ProviderFormValues;

vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({
    onSubmit,
  }: {
    onSubmit: (values: ProviderFormValues) => void;
  }) => {
    return (
      <form
        id="provider-form"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit(mockFormValues);
        }}
      />
    );
  },
}));

describe("AddProviderDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockFormValues = {
      name: "Test Provider",
      websiteUrl: "https://provider.example.com",
      settingsConfig: JSON.stringify({ env: {}, config: {} }),
      meta: {
        custom_endpoints: {
          "https://api.new-endpoint.com": {
            url: "https://api.new-endpoint.com",
            addedAt: 1,
          },
        },
      },
    };
    universalApiMocks.saveAndSync.mockResolvedValue(true);
    universalProtocolMocks.preflightCodex.mockResolvedValue({
      provider: {
        id: "codex-draft",
        name: "Test Provider",
        settingsConfig: {},
      },
      records: [],
      receiptIds: ["receipt-model-a"],
      protocolApplied: false,
    });
    universalProtocolMocks.prepareCodex.mockResolvedValue({
      digest: "codex-provider-digest",
      sourceProviderId: "generated-provider-id",
      responsesModels: ["model-a"],
      chatModels: [],
      plan: { kind: "single", transport: "open_ai_responses" },
    });
    universalProtocolMocks.commitCodex.mockResolvedValue({
      preview: {
        digest: "codex-provider-digest",
        sourceProviderId: "generated-provider-id",
        responsesModels: ["model-a"],
        chatModels: [],
        plan: { kind: "single", transport: "open_ai_responses" },
      },
      projections: [],
      status: "committed",
    });
    universalFormSubmission.current = {
      id: "universal-mixed",
      name: "Mixed Gateway",
      providerType: "newapi",
      baseUrl: "https://gateway.example/v1",
      apiKey: "secret-key",
      apps: { claude: false, codex: true, gemini: false },
      models: { codex: { model: "qwen3.8" } },
    };
    universalProtocolMocks.preflight.mockResolvedValue({
      provider: {
        id: "universal-codex-universal-mixed",
        name: "Mixed Gateway",
        settingsConfig: {},
      },
      records: [],
      receiptIds: ["receipt-qwen"],
      protocolApplied: false,
    });
    universalProtocolMocks.prepare.mockResolvedValue({
      digest: "universal-digest",
      universalProviderId: "universal-mixed",
      codex: {
        digest: "codex-digest",
        sourceProviderId: "universal-codex-universal-mixed",
        responsesModels: ["qwen3.8"],
        chatModels: [],
        plan: { kind: "single", transport: "open_ai_responses" },
      },
    });
    universalProtocolMocks.commit.mockResolvedValue({
      preview: {
        digest: "universal-digest",
        universalProviderId: "universal-mixed",
        codex: null,
      },
    });
  });

  it("普通 Codex 新增使用已有 receipt 提交 Provider Set 且不调用旧新增接口", async () => {
    const user = userEvent.setup();
    const handleSubmit = vi.fn().mockResolvedValue(undefined);
    const handleOpenChange = vi.fn();
    mockFormValues = {
      name: "Mixed Codex Relay",
      websiteUrl: "https://relay.example",
      settingsConfig: JSON.stringify({
        auth: { OPENAI_API_KEY: "secret" },
        apiFormat: "openai_responses",
        config:
          'model = "model-a"\nmodel_provider = "relay"\n[model_providers.relay]\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
        modelCatalog: { models: [{ model: "model-a" }] },
      }),
      meta: { apiFormat: "openai_responses" },
      protocolProbeReceiptIds: ["receipt-model-a"],
    };

    render(
      <AddProviderDialog
        open
        onOpenChange={handleOpenChange}
        appId="codex"
        onSubmit={handleSubmit}
      />,
    );

    await user.click(screen.getByRole("tab", { name: "单独接入模型源" }));
    await user.click(screen.getByRole("button", { name: "common.add" }));

    await waitFor(() =>
      expect(universalProtocolMocks.commitCodex).toHaveBeenCalledWith(
        expect.objectContaining({
          id: expect.any(String),
          name: "Mixed Codex Relay",
        }),
        ["receipt-model-a"],
        "codex-provider-digest",
        "accept_single",
      ),
    );
    expect(universalProtocolMocks.preflightCodex).not.toHaveBeenCalled();
    expect(handleSubmit).not.toHaveBeenCalled();
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });

  it("普通 Codex 混合结果只确认一次后端拆分事务", async () => {
    const user = userEvent.setup();
    const handleSubmit = vi.fn().mockResolvedValue(undefined);
    const handleOpenChange = vi.fn();
    mockFormValues = {
      name: "Mixed Codex Relay",
      settingsConfig: JSON.stringify({
        auth: { OPENAI_API_KEY: "secret" },
        apiFormat: "openai_responses",
        config:
          'model = "model-a"\nmodel_provider = "relay"\n[model_providers.relay]\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
        modelCatalog: {
          models: [{ model: "model-a" }, { model: "model-b" }],
        },
      }),
      meta: { apiFormat: "openai_responses" },
      protocolProbeReceiptIds: ["receipt-model-a", "receipt-model-b"],
    };
    universalProtocolMocks.prepareCodex.mockResolvedValueOnce({
      digest: "split-digest",
      sourceProviderId: "generated-provider-id",
      responsesModels: ["model-a"],
      chatModels: ["model-b"],
      plan: {
        kind: "split",
        responses_provider_id: "generated-provider-id--ccsm-responses",
        chat_provider_id: "generated-provider-id--ccsm-chat",
      },
    });

    render(
      <AddProviderDialog
        open
        onOpenChange={handleOpenChange}
        appId="codex"
        onSubmit={handleSubmit}
      />,
    );

    await user.click(screen.getByRole("tab", { name: "单独接入模型源" }));
    await user.click(screen.getByRole("button", { name: "common.add" }));
    expect(await screen.findByText("Responses 模型")).toBeInTheDocument();
    expect(screen.getAllByText("model-a").length).toBeGreaterThan(0);
    expect(screen.getAllByText("model-b").length).toBeGreaterThan(0);

    await user.click(
      screen.getByRole("button", { name: "确认按协议拆分" }),
    );

    await waitFor(() =>
      expect(universalProtocolMocks.commitCodex).toHaveBeenCalledWith(
        expect.objectContaining({ name: "Mixed Codex Relay" }),
        ["receipt-model-a", "receipt-model-b"],
        "split-digest",
        "confirm_split",
      ),
    );
    expect(universalProtocolMocks.commitCodex).toHaveBeenCalledOnce();
    expect(handleSubmit).not.toHaveBeenCalled();
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });

  it("新增模型源使用 Provider Set 事务而不是旧 saveAndSync", async () => {
    const handleOpenChange = vi.fn();

    render(
      <AddProviderDialog
        open
        onOpenChange={handleOpenChange}
        appId="codex"
        onSubmit={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "添加模型源" }));
    fireEvent.click(
      screen.getByRole("button", { name: "submit-universal-provider" }),
    );

    await waitFor(() =>
      expect(universalProtocolMocks.commit).toHaveBeenCalledWith(
        universalFormSubmission.current,
        ["receipt-qwen"],
        "universal-digest",
        "accept_single",
      ),
    );
    expect(universalApiMocks.saveAndSync).not.toHaveBeenCalled();
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });

  it("使用 ProviderForm 返回的自定义端点", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);
    const handleOpenChange = vi.fn();

    render(
      <AddProviderDialog
        open
        onOpenChange={handleOpenChange}
        appId="claude"
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "common.add",
      }),
    );

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    const submitted = handleSubmit.mock.calls[0][0];
    expect(submitted.meta?.custom_endpoints).toEqual(
      mockFormValues.meta?.custom_endpoints,
    );
    expect(handleOpenChange).toHaveBeenCalledWith(false);
  });

  it("在缺少自定义端点时回退到配置中的 baseUrl", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    mockFormValues = {
      name: "Base URL Provider",
      websiteUrl: "",
      settingsConfig: JSON.stringify({
        env: { ANTHROPIC_BASE_URL: "https://claude.base" },
        config: {},
      }),
    };

    render(
      <AddProviderDialog
        open
        onOpenChange={vi.fn()}
        appId="claude"
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "common.add",
      }),
    );

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    const submitted = handleSubmit.mock.calls[0][0];
    expect(submitted.meta?.custom_endpoints).toEqual({
      "https://claude.base": {
        url: "https://claude.base",
        addedAt: expect.any(Number),
        lastUsed: undefined,
      },
    });
  });

  it("新建 Grok Build 自定义供应商时不补默认 Grok 图标", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    mockFormValues = {
      name: "tes 1",
      websiteUrl: "",
      icon: "",
      iconColor: "",
      settingsConfig: JSON.stringify({
        config: `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://grok.example.com/v1"
name = "tes 1"
api_key = "secret"
api_backend = "responses"
context_window = 500000
`,
      }),
    };

    render(
      <AddProviderDialog
        open
        onOpenChange={vi.fn()}
        appId="grokbuild"
        onSubmit={handleSubmit}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "common.add" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    const submitted = handleSubmit.mock.calls[0][0];
    expect(submitted.icon).toBeUndefined();
    expect(submitted.iconColor).toBeUndefined();
  });
});
