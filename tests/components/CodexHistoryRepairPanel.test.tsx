import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexHistoryRepairPanel } from "@/components/sessions/CodexHistoryRepairPanel";
import { proxyApi } from "@/lib/api/proxy";

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    unlockCodexModelPicker: vi.fn(),
    listCodexHistorySessions: vi.fn(),
    readCodexHistorySession: vi.fn(),
    repairCodexHistoryVisibility: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    error: vi.fn(),
  },
}));

// 构造最小历史列表返回值，用于验证前端参数而不依赖真实 SQLite。
function historyListFixture() {
  return {
    codexHome: "C:\\Users\\sunda\\.codex",
    stateDbPath: "C:\\Users\\sunda\\.codex\\state_5.sqlite",
    activeDbKind: "codex_root",
    liveConfigModelProvider: "openai",
    targetProviderCandidates: ["openai", "custom"],
    sourceCounts: [{ value: "vscode", count: 7 }],
    providerCounts: [
      { value: "openai", count: 4 },
      { value: "custom", count: 3 },
    ],
    totalMatched: 0,
    items: [],
    skippedReason: null,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  vi.mocked(proxyApi.listCodexHistorySessions).mockResolvedValue(
    historyListFixture(),
  );
  vi.mocked(proxyApi.readCodexHistorySession).mockResolvedValue({
    codexHome: "C:\\Users\\sunda\\.codex",
    stateDbPath: "C:\\Users\\sunda\\.codex\\state_5.sqlite",
    activeDbKind: "codex_root",
    session: null,
    messages: [],
    rolloutPath: null,
    skippedReason: null,
  });
});

describe("CodexHistoryRepairPanel", () => {
  it("loads all projects by default even when opened from a selected session", async () => {
    render(
      <CodexHistoryRepairPanel
        initialProjectPath="C:\\Users\\sunda\\Documents\\ACPs 2\\ACPs Agent Adapter"
        showAutomationGuide
      />,
    );

    await waitFor(() => {
      expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledTimes(1);
    });

    expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledWith(
      expect.objectContaining({
        projectPath: null,
      }),
    );
    expect(
      screen.getByPlaceholderText("默认空；为空时修复所有项目"),
    ).toHaveValue("");
    expect(screen.getByText(/未限制项目；可手动带入/)).toBeInTheDocument();
  });
});
