import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { providersApi } from "@/lib/api/providers";
import { CodexMultiRouterWizard } from "./CodexMultiRouterWizard";

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: () => ({
    accounts: [],
    hasAnyAccount: false,
    isLoadingStatus: false,
  }),
}));

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getCodexMultiRouterRevision: vi.fn().mockResolvedValue("revision-1"),
    previewCodexMultiRouterMigration: vi.fn().mockResolvedValue({
      schemaVersion: 2,
      providerId: "router-b",
      expectedRevision: "revision-1",
      planToken: "opaque-token",
      diff: {
        removedRouteFields: ["upstream.apiFormat"],
        createdProviderIds: [],
        changedRouteIds: ["router-b-route"],
      },
      warnings: [],
      generatedProviders: [],
    }),
    applyCodexMultiRouterMigration: vi.fn(),
    getAll: vi.fn(),
    update: vi.fn(),
    add: vi.fn(),
  },
}));

function renderWizard(
  providers: Provider[],
  options?: { mode?: "create" | "edit"; planId?: string },
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <CodexMultiRouterWizard
        open
        providers={providers}
        mode={options?.mode ?? "create"}
        planId={options?.planId}
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

  it("edits the explicitly selected plan instead of the first cached routing plan", () => {
    const routingPlan = (id: string, name: string): Provider => ({
      id,
      name,
      category: "custom",
      settingsConfig: {
        codexRouting: { enabled: true, routes: [{ id: `${id}-route` }] },
        modelCatalog: { models: [{ model: `${id}-model` }] },
      },
    });

    renderWizard(
      [
        routingPlan("router-a", "旧方案 A"),
        routingPlan("router-b", "目标方案 B"),
      ],
      { mode: "edit", planId: "router-b" },
    );

    expect(screen.getByText("正在编辑：目标方案 B")).toBeVisible();
    expect(screen.getByText("router-b")).toBeVisible();
    expect(screen.queryByText("正在编辑：旧方案 A")).not.toBeInTheDocument();
  });

  it("requires an explicit redacted migration preview before editing a v1 plan", async () => {
    const legacyPlan: Provider = {
      id: "legacy-plan",
      name: "Legacy Plan",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "must-not-render" },
        codexRouting: {
          enabled: true,
          routes: [
            {
              id: "legacy-route",
              targetProviderId: "qwen",
              match: { models: ["qwen3.8"] },
              upstream: {
                apiFormat: "openai_chat",
                apiKey: "legacy-secret",
                auth: { source: "provider_config" },
              },
            },
          ],
        },
      },
    };

    renderWizard([legacyPlan], { mode: "edit", planId: legacyPlan.id });

    expect(
      await screen.findByRole("heading", {
        name: "编辑前迁移旧 MultiRouter",
      }),
    ).toBeVisible();
    expect(providersApi.getCodexMultiRouterRevision).toHaveBeenCalledWith(
      legacyPlan.id,
    );
    expect(screen.queryByText("legacy-secret")).not.toBeInTheDocument();
    expect(screen.queryByText("must-not-render")).not.toBeInTheDocument();
  });

  it("keeps provider-owned protocol and hosted-tool controls out of source selection", () => {
    renderWizard([
      {
        id: "third-party",
        name: "Third party source",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://example.invalid/v1",
          auth: { OPENAI_API_KEY: "test-only" },
          modelCatalog: { models: [{ model: "third-party-model" }] },
        },
      },
    ]);

    expect(screen.queryByText("OpenAI Hosted Tools")).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("Third party source API 格式"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "配置 Third party source" }),
    ).toBeVisible();
  });

  it("reviews schema v2 route policy without inherited endpoint, protocol, or capabilities", () => {
    renderWizard([
      {
        id: "qwen-provider",
        name: "Qwen Provider",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://secret-upstream.invalid/v1",
          apiFormat: "openai_chat",
          auth: { OPENAI_API_KEY: "must-not-render" },
          modelCatalog: {
            models: [
              {
                model: "qwen3.8",
                apiFormat: "openai_responses",
                codexCache: { cacheMode: "qwen_context_cache" },
              },
            ],
          },
        },
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "选择模型并预览路由" }));

    expect(screen.getByText("qwen-provider")).toBeVisible();
    expect(screen.getByText(/Route 不保存这些字段/)).toBeVisible();
    expect(screen.queryByText("openai_chat")).not.toBeInTheDocument();
    expect(screen.queryByText("openai_responses")).not.toBeInTheDocument();
    expect(
      screen.queryByText("https://secret-upstream.invalid/v1"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("must-not-render")).not.toBeInTheDocument();
  });
});
