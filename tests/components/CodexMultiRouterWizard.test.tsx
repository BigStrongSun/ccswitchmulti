import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import type { Provider } from "@/types";
import { CodexMultiRouterWizard } from "@/components/codex/CodexMultiRouterWizard";
import { CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY } from "@/lib/codexMultiRouterWizard";
import { providersApi } from "@/lib/api/providers";

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    add: vi.fn(),
    update: vi.fn(),
  },
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
}));

// 构造最小 Codex provider，避免 UI 测试依赖真实数据库返回结构。
function provider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: overrides.id ?? "deepseek",
    name: overrides.name ?? "DeepSeek",
    category: overrides.category,
    settingsConfig: overrides.settingsConfig ?? {
      base_url: "https://api.deepseek.com/v1",
      auth: { OPENAI_API_KEY: "sk-test" },
      modelCatalog: { models: [{ model: "deepseek-chat" }] },
    },
    meta: overrides.meta,
  };
}

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
});

describe("CodexMultiRouterWizard", () => {
  it("opens, navigates steps, and stores dismissed flag when skipped", () => {
    const onOpenChange = vi.fn();

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={onOpenChange}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    expect(screen.getAllByText("理解 MultiRouter").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getAllByText("创建模型源").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    expect(localStorage.getItem(CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY)).toBe(
      "true",
    );
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("stays in needSources state when advancing without model sources", () => {
    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));

    expect(screen.getByText("状态机：needSources")).toBeInTheDocument();
    expect(screen.getByText(/请先添加一个普通 Codex/)).toBeInTheDocument();
  });

  it("moves to saveFailed state when publishing the generated plan fails", async () => {
    vi.mocked(providersApi.add).mockRejectedValueOnce(new Error("db locked"));

    renderWithQueryClient(
      <CodexMultiRouterWizard
        open
        providers={[provider()]}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存并发布" }));
    const publishButtons = screen.getAllByRole("button", {
      name: "保存并发布",
    });
    fireEvent.click(publishButtons[publishButtons.length - 1]);

    expect(await screen.findByText("状态机：saveFailed")).toBeInTheDocument();
    expect(screen.getByText("db locked")).toBeInTheDocument();
  });
});
