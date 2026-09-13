import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  CodexProviderEditorSnapshot,
  CodexProviderProtocolPreflightOutcome,
  CodexProviderSetCommitOutcome,
  CodexProviderSetPreview,
} from "@/lib/api/protocol-compatibility";
import type { Provider } from "@/types";

import { useCodexProviderSetSave } from "./useCodexProviderSetSave";

const protocolCompatibilityMocks = vi.hoisted(() => ({
  preflight: vi.fn(),
  prepare: vi.fn(),
  commit: vi.fn(),
  editorSnapshot: vi.fn(),
  restore: vi.fn(),
}));

const providerApiMocks = vi.hoisted(() => ({
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api/protocol-compatibility", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("@/lib/api/protocol-compatibility")
  >()),
  preflightCodexProviderProtocolCompatibility:
    protocolCompatibilityMocks.preflight,
  prepareCodexProviderSet: protocolCompatibilityMocks.prepare,
  commitCodexProviderSet: protocolCompatibilityMocks.commit,
  getCodexProviderEditorSnapshot: protocolCompatibilityMocks.editorSnapshot,
  restoreCodexProviderProtocolEvidence: protocolCompatibilityMocks.restore,
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    providersApi: {
      ...actual.providersApi,
      updateTrayMenu: providerApiMocks.updateTrayMenu,
    },
  };
});

const provider: Provider = {
  id: "third-party-provider",
  name: "Third-party relay",
  category: "custom",
  settingsConfig: {
    auth: { OPENAI_API_KEY: "sk-test" },
    modelCatalog: { models: [{ model: "model-a" }] },
  },
  meta: { apiFormat: "openai_responses" },
};

const adaptation = {
  persistence: "single" as const,
  status: "ready" as const,
  effectiveTransport: "open_ai_responses" as const,
  models: [],
};

const restoredOutcome: CodexProviderProtocolPreflightOutcome = {
  provider,
  receiptIds: ["restored-receipt"],
  records: [],
  observations: [],
  protocolApplied: false,
  adaptationPreview: adaptation,
};

const preview: CodexProviderSetPreview = {
  digest: "prepared-digest",
  sourceProviderId: provider.id,
  responsesModels: ["model-a"],
  chatModels: [],
  plan: { kind: "single", transport: "open_ai_responses" },
};

const snapshot: CodexProviderEditorSnapshot = {
  logicalProvider: provider,
  adaptation,
};

const commitOutcome: CodexProviderSetCommitOutcome = {
  preview,
  snapshot,
  projections: [],
  status: "committed",
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useCodexProviderSetSave", () => {
  beforeEach(() => {
    protocolCompatibilityMocks.preflight.mockReset();
    protocolCompatibilityMocks.prepare.mockReset();
    protocolCompatibilityMocks.commit.mockReset();
    protocolCompatibilityMocks.editorSnapshot.mockReset();
    protocolCompatibilityMocks.restore.mockReset();
    providerApiMocks.updateTrayMenu.mockReset();
    protocolCompatibilityMocks.prepare.mockResolvedValue(preview);
    protocolCompatibilityMocks.commit.mockResolvedValue(commitOutcome);
    providerApiMocks.updateTrayMenu.mockResolvedValue(undefined);
  });

  it("restores persisted evidence before saving an automatic Provider whose UI receipt is missing", async () => {
    protocolCompatibilityMocks.restore.mockResolvedValue(restoredOutcome);
    const { result } = renderHook(() => useCodexProviderSetSave(), {
      wrapper: createWrapper(),
    });
    let savePromise!: Promise<void>;

    try {
      act(() => {
        savePromise = result.current.persistCodexProviderSet(provider);
        savePromise.catch(() => undefined);
      });

      await waitFor(() =>
        expect(protocolCompatibilityMocks.restore).toHaveBeenCalledWith(
          provider,
        ),
      );
      await act(async () => {
        await savePromise;
      });

      expect(protocolCompatibilityMocks.preflight).not.toHaveBeenCalled();
      expect(protocolCompatibilityMocks.prepare).toHaveBeenCalledWith(
        provider,
        ["restored-receipt"],
      );
      expect(protocolCompatibilityMocks.commit).toHaveBeenCalledWith(
        provider,
        ["restored-receipt"],
        "prepared-digest",
        "accept_auto",
      );
      expect(result.current.workflow.state.phase).toBe("committed");
    } finally {
      act(() => result.current.workflow.cancel());
    }
  });
});
