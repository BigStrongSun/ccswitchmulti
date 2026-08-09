import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import React from "react";
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

describe("CodexMultiRouterWizard V2 subagent flow", () => {
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
