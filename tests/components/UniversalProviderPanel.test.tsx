import {
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
const formSubmission = vi.hoisted(() => ({
  current: null as UniversalProvider | null,
}));
const toastMocks = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));

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
  receiptIds: ["receipt-qwen"],
  protocolApplied: false,
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

vi.mock("@/lib/api", () => ({ universalProvidersApi: apiMocks }));
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
  }: {
    onSaveAndSync: (provider: UniversalProvider) => void | Promise<void>;
  }) => (
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
  ),
}));

describe("UniversalProviderPanel Provider Set persistence", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.getAll.mockResolvedValue({});
    protocolMocks.preflight.mockResolvedValue(preflightOutcome);
    protocolMocks.prepare.mockResolvedValue(singlePreview);
    protocolMocks.commit.mockResolvedValue({ preview: singlePreview });
    formSubmission.current = provider;
  });

  it("deep-probes, prepares, and directly commits a Single plan", async () => {
    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
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
      "accept_single",
    );
    expect(apiMocks.saveAndSync).not.toHaveBeenCalled();
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

  it("shows backend model groups and commits Split only after confirmation", async () => {
    protocolMocks.prepare.mockResolvedValueOnce(splitPreview);

    render(<UniversalProviderPanel />);
    fireEvent.click(
      screen.getByRole("button", { name: "invoke-save-and-sync" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "按协议自动拆分",
    });
    expect(within(dialog).getByText("qwen3.8")).toBeInTheDocument();
    expect(within(dialog).getByText("deepseek-v4")).toBeInTheDocument();
    expect(protocolMocks.commit).not.toHaveBeenCalled();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "确认按协议拆分" }),
    );

    await waitFor(() => expect(callbackOutcome.resolved).toHaveBeenCalled());
    expect(protocolMocks.commit).toHaveBeenCalledTimes(1);
    expect(protocolMocks.commit).toHaveBeenCalledWith(
      provider,
      ["receipt-qwen"],
      "universal-digest",
      "confirm_split",
    );
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
