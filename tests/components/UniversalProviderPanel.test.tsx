import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UniversalProviderPanel } from "@/components/universal/UniversalProviderPanel";
import type {
  CodexProviderSetPreview,
  CodexProviderProtocolPreflightOutcome,
  UniversalProviderSetPreview,
} from "@/lib/api/protocol-compatibility";
import type { UniversalProvider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  getAll: vi.fn().mockResolvedValue({}),
  saveAndSync: vi.fn(),
  upsert: vi.fn(),
  delete: vi.fn(),
  sync: vi.fn(),
  updateTrayMenu: vi.fn().mockResolvedValue(true),
  retryCodexMultiRouterProjection: vi.fn().mockResolvedValue({
    state: "ready",
  }),
}));
const protocolMocks = vi.hoisted(() => ({
  preflight: vi.fn(),
  prepare: vi.fn(),
  commit: vi.fn(),
}));
const callbackOutcome = vi.hoisted(() => ({
  resolved: vi.fn(),
  rejected: vi.fn(),
}));
const queryClientMocks = vi.hoisted(() => ({
  invalidateQueries: vi.fn().mockResolvedValue(undefined),
}));
const formSubmission = vi.hoisted(() => ({
  current: null as UniversalProvider | null,
}));
const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));

const provider: UniversalProvider = {
  id: "universal-contract",
  name: "Contract Gateway",
  providerType: "newapi",
  baseUrl: "https://gateway.example/v1",
  apiKey: "secret-key",
  apps: { claude: true, codex: true, gemini: true },
  models: { codex: { model: "qwen3.8" } },
};

const preflightOutcome: CodexProviderProtocolPreflightOutcome = {
  provider: {
    id: "universal-codex-universal-contract",
    name: "Contract Gateway",
    settingsConfig: {},
  },
  records: [],
  observations: [],
  receiptIds: ["receipt-qwen"],
  protocolApplied: false,
  adaptationPreview: {
    persistence: "single",
    status: "ready",
    effectiveTransport: "open_ai_responses",
    models: [],
  },
};

function preview(
  codex: CodexProviderSetPreview | null,
): UniversalProviderSetPreview {
  return {
    digest: "universal-digest",
    universalProviderId: provider.id,
    codex,
  };
}

const singlePreview = preview({
  digest: "codex-single",
  sourceProviderId: "universal-codex-universal-contract",
  responsesModels: ["qwen3.8"],
  chatModels: [],
  plan: { kind: "single", transport: "open_ai_responses" },
});

const splitPreview = preview({
  digest: "codex-split",
  sourceProviderId: "universal-codex-universal-contract",
  responsesModels: ["qwen3.8"],
  chatModels: ["deepseek-v4"],
  plan: {
    kind: "split",
    responses_provider_id: "universal-codex-universal-contract--ccsm-responses",
    chat_provider_id: "universal-codex-universal-contract--ccsm-chat",
  },
});

vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return {
    ...actual,
    useQueryClient: () => queryClientMocks,
  };
});
vi.mock("@/lib/api", () => ({
  universalProvidersApi: apiMocks,
  providersApi: {
    updateTrayMenu: apiMocks.updateTrayMenu,
    retryCodexMultiRouterProjection:
      apiMocks.retryCodexMultiRouterProjection,
  },
}));
vi.mock("@/lib/api/protocol-compatibility", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/api/protocol-compatibility")>();
  return {
    ...actual,
    preflightUniversalCodexProtocolCompatibility: protocolMocks.preflight,
    prepareUniversalProviderSet: protocolMocks.prepare,
    commitUniversalProviderSet: protocolMocks.commit,
  };
});
vi.mock("sonner", () => ({ toast: toastMocks }));
vi.mock("@/components/universal/UniversalProviderCard", () => ({
  UniversalProviderCard: () => null,
}));
vi.mock("@/components/universal/UniversalProviderFormModal", () => ({
  UniversalProviderFormModal: ({
    onSaveAndSync,
    onSave,
  }: {
    onSaveAndSync: (provider: UniversalProvider) => void | Promise<void>;
    onSave?: (provider: UniversalProvider) => void | Promise<void>;
  }) => (
    <>
      {onSave ? <span>legacy-save-present</span> : null}
      <button
        type="button"
        onClick={() =>
          Promise.resolve(onSaveAndSync(formSubmission.current!)).then(
            callbackOutcome.resolved,
            callbackOutcome.rejected,
          )
        }
      >
        invoke-save-and-sync
      </button>
    </>
  ),
}));

describe("UniversalProviderPanel Provider Set persistence", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.getAll.mockResolvedValue({});
    protocolMocks.preflight.mockResolvedValue(preflightOutcome);
    protocolMocks.prepare.mockResolvedValue(singlePreview);
    protocolMocks.commit.mockResolvedValue({
      preview: singlePreview,
      codexSnapshot: null,
      status: "committed",
      projectionErrorCode: null,
      projections: [],
    });
    formSubmission.current = provider;
  });

  it("deep-probes, prepares, and directly commits a Single plan", async () => {
    render(<UniversalProviderPanel />);
    expect(screen.queryByText("legacy-save-present")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );
    expect(protocolMocks.preflight).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", { name: "确认测试" }),
    );

    await waitFor(() => expect(callbackOutcome.resolved).toHaveBeenCalled());
    expect(protocolMocks.preflight).toHaveBeenCalledWith(
      provider,
      expect.any(Function),
    );
    expect(protocolMocks.prepare).toHaveBeenCalledWith(provider, [
      "receipt-qwen",
    ]);
    expect(protocolMocks.commit).toHaveBeenCalledWith(
      provider,
      ["receipt-qwen"],
      "universal-digest",
      "accept_auto",
    );
    expect(apiMocks.saveAndSync).not.toHaveBeenCalled();
    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["providers"],
    });
    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["codex-provider-adaptation-summaries"],
    });
    expect(apiMocks.updateTrayMenu).toHaveBeenCalledTimes(1);
  });

  it("reports a committed projection warning instead of a save failure or success", async () => {
    protocolMocks.commit.mockResolvedValueOnce({
      preview: singlePreview,
      codexSnapshot: null,
      status: "committed_with_projection_error",
      projectionErrorCode: "codex_provider_set_live_projection_failed",
      projections: [
        {
          schemaVersion: 2,
          routerProviderId: "router-needs-refresh",
          state: "pending",
          dependencyFingerprint: "pending-fingerprint",
          generatedAt: "2026-08-30T00:00:00Z",
          warnings: [],
          routes: [],
          lastErrorCode: "projection_publish_failed",
        },
      ],
    });

    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "确认测试" }));

    await waitFor(() => expect(callbackOutcome.resolved).toHaveBeenCalled());
    expect(toastMocks.warning).toHaveBeenCalledWith(
      expect.stringContaining("已保存"),
      expect.objectContaining({
        action: expect.objectContaining({ label: "重新投影" }),
      }),
    );
    expect(toastMocks.success).not.toHaveBeenCalled();
    expect(toastMocks.error).not.toHaveBeenCalled();

    const warningOptions = toastMocks.warning.mock.calls[0]?.[1] as {
      action?: { onClick?: () => void | Promise<void> };
    };
    await act(async () => {
      await warningOptions.action?.onClick?.();
    });
    expect(apiMocks.retryCodexMultiRouterProjection).toHaveBeenCalledWith(
      "router-needs-refresh",
    );
  });

  it("keeps advanced manual protocol separate from automatic probing", async () => {
    const manualProvider: UniversalProvider = {
      ...provider,
      meta: {
        codexProtocolMode: "manual",
        apiFormat: "openai_chat",
      },
    };
    formSubmission.current = manualProvider;
    protocolMocks.prepare.mockResolvedValueOnce(singlePreview);

    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );

    await waitFor(() => expect(callbackOutcome.resolved).toHaveBeenCalled());
    expect(protocolMocks.preflight).not.toHaveBeenCalled();
    expect(protocolMocks.prepare).toHaveBeenCalledWith(manualProvider, []);
    expect(protocolMocks.commit).toHaveBeenCalledWith(
      expect.objectContaining({
        meta: expect.objectContaining({ codexProtocolMode: "manual" }),
      }),
      expect.any(Array),
      "universal-digest",
      "confirm_manual",
    );
  });

  it("commits an automatic Split without a second split confirmation", async () => {
    protocolMocks.prepare.mockResolvedValueOnce(splitPreview);

    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "确认测试" }),
    );

    await waitFor(() => expect(callbackOutcome.resolved).toHaveBeenCalled());
    expect(protocolMocks.commit).toHaveBeenCalledTimes(1);
    expect(protocolMocks.commit).toHaveBeenCalledWith(
      provider,
      ["receipt-qwen"],
      "universal-digest",
      "accept_auto",
    );
    expect(
      screen.queryByRole("button", { name: "确认按协议拆分" }),
    ).not.toBeInTheDocument();
    expect(apiMocks.saveAndSync).not.toHaveBeenCalled();
  });

  it("keeps Blocked plans unsaved and lets the form return for adjustment", async () => {
    const blockedPreview = preview({
      digest: "codex-blocked",
      sourceProviderId: "universal-codex-universal-contract",
      responsesModels: [],
      chatModels: [],
      plan: {
        kind: "blocked",
        models: [
          {
            model: "qwen3.8",
            upstreamModel: "qwen3.8",
            reason: "probe_not_verified",
          },
        ],
      },
    });
    protocolMocks.prepare.mockResolvedValueOnce(blockedPreview);

    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "确认测试" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "暂时无法保存",
    });
    expect(protocolMocks.commit).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "返回调整模型" }),
    );

    await waitFor(() => expect(callbackOutcome.rejected).toHaveBeenCalled());
    expect(apiMocks.saveAndSync).not.toHaveBeenCalled();
  });

  it("keeps a failed probe visible and retries without losing the pending save", async () => {
    protocolMocks.preflight.mockRejectedValueOnce(
      new Error("database failure"),
    );

    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "确认测试" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Codex 兼容性深度探测",
    });
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "database failure",
    );
    expect(callbackOutcome.rejected).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "重新探测" }));

    await waitFor(() => expect(callbackOutcome.resolved).toHaveBeenCalled());
    expect(protocolMocks.preflight).toHaveBeenCalledTimes(2);
    expect(toastMocks.error).not.toHaveBeenCalled();
    expect(apiMocks.saveAndSync).not.toHaveBeenCalled();
  });
});
