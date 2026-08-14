import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { CodexMultiRouterWizard } from "./CodexMultiRouterWizard";

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: () => ({
    accounts: [],
    hasAnyAccount: false,
    isLoadingStatus: false,
  }),
}));

function renderWizard(providers: Provider[]) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <CodexMultiRouterWizard
        open
        providers={providers}
        onOpenChange={vi.fn()}
        onCreateProvider={vi.fn()}
        onOpenProviderConfig={vi.fn()}
        onOpenWorkspace={vi.fn()}
        onEnablePlan={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("CodexMultiRouterWizard", () => {
  it("keeps V1 and V2 configuration out of the four-stage routing wizard", () => {
    renderWizard([
      {
        id: "codex-deepseek",
        name: "DeepSeek",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [
              { model: "deepseek-v4-flash" },
              { model: "deepseek-v4-pro" },
            ],
          },
        },
      },
    ]);

    expect(
      screen.queryByRole("button", { name: /Sub-Agent V1/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Sub-Agent V2/ }),
    ).not.toBeInTheDocument();
  });

  it("presents MultiRouter setup as four user tasks", () => {
    renderWizard([
      {
        id: "codex-deepseek",
        name: "DeepSeek",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [{ model: "deepseek-v4-flash" }],
          },
        },
      },
    ]);

    expect(
      screen.getByRole("button", { name: "选择模型源" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "自动准备与验证" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "选择模型并预览路由" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "启用并验证" }),
    ).toBeInTheDocument();

    expect(
      screen.queryByRole("button", { name: "理解 MultiRouter" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "获取模型列表" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "处理重名模型" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "保存并发布" }),
    ).not.toBeInTheDocument();
  });

  it("does not require users to choose subagent models in the main wizard", () => {
    renderWizard([
      {
        id: "codex-deepseek",
        name: "DeepSeek",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: {
            models: [
              { model: "deepseek-v4-flash" },
              { model: "deepseek-v4-pro" },
            ],
          },
        },
      },
    ]);

    expect(screen.queryByText("子 Agent 候选")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/选择并排序最多 5 个子 Agent 候选模型/),
    ).not.toBeInTheDocument();
  });
});
