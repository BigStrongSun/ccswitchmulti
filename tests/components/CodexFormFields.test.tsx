import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildSplitCodexProviderSuggestionForFetchedModels,
  CodexFormFields,
  splitFetchedModelsByLikelyCodexProtocol,
} from "@/components/providers/forms/CodexFormFields";
import {
  fetchModelsForConfig,
  probeCodexChatForConfig,
  probeCodexResponsesForConfig,
} from "@/lib/api/model-fetch";
import type { CodexCatalogModel, CodexRoutingConfig } from "@/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
  probeCodexChatForConfig: vi.fn(),
  probeCodexResponsesForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/components/ui/form", () => ({
  FormLabel: ({ children }: { children: ReactNode }) => (
    <label>{children}</label>
  ),
}));

beforeEach(() => {
  vi.mocked(fetchModelsForConfig).mockReset();
  vi.mocked(probeCodexChatForConfig).mockReset();
  vi.mocked(probeCodexResponsesForConfig).mockReset();
  Element.prototype.scrollIntoView = vi.fn();
});

// 构造协议探测的成功返回，供并发池测试精确控制每个模型的完成顺序。
function createProbeResult(
  model: string,
  detail = "ok",
): {
  ok: true;
  status: number;
  url: string;
  model: string;
  detail: string;
} {
  return {
    ok: true,
    status: 200,
    url: "https://api.thirdparty.example/v1/probe",
    model,
    detail,
  };
}

function renderRoutingHarness(
  initialRouting?: CodexRoutingConfig,
  options: { shouldShowSpeedTest?: boolean } = {},
) {
  const onRoutingChange = vi.fn();
  let latestRouting: CodexRoutingConfig = initialRouting ?? {
    enabled: true,
    defaultRouteId: "",
    routes: [],
  };

  function Harness() {
    const [routing, setRouting] = useState<CodexRoutingConfig>(latestRouting);

    // 测试壳同步保存最新 route 配置，模拟 ProviderForm 对受控字段的回写。
    const handleRoutingChange = (next: CodexRoutingConfig) => {
      latestRouting = next;
      onRoutingChange(next);
      setRouting(next);
    };

    return (
      <CodexFormFields
        codexApiKey="sk-test"
        onApiKeyChange={vi.fn()}
        category="custom"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest={options.shouldShowSpeedTest ?? true}
        codexBaseUrl="https://api.example.com"
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        takeoverEnabled={true}
        onTakeoverEnabledChange={vi.fn()}
        apiFormat="openai_chat"
        onApiFormatChange={vi.fn()}
        codexRouting={routing}
        onCodexRoutingChange={handleRoutingChange}
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    );
  }

  return {
    ...render(<Harness />),
    onRoutingChange,
    latestRouting: () => latestRouting,
  };
}

function renderCatalogHarness(
  initialCatalog: CodexCatalogModel[],
  options: {
    shouldShowSpeedTest?: boolean;
    providerName?: string;
    partnerPromotionKey?: string;
    baseUrl?: string;
    apiKey?: string;
    planAccessKeyId?: string;
    planSecretAccessKey?: string;
    takeoverEnabled?: boolean;
    onProviderSplitSuggestionChange?: ReturnType<typeof vi.fn>;
  } = {},
) {
  const onCatalogChange = vi.fn();
  const onApiFormatChange = vi.fn();
  let latestCatalog = initialCatalog;

  function Harness() {
    const [catalog, setCatalog] = useState<CodexCatalogModel[]>(initialCatalog);

    // 测试壳模拟 ProviderForm 对 modelCatalog 的受控回写。
    const handleCatalogChange = (next: CodexCatalogModel[]) => {
      latestCatalog = next;
      onCatalogChange(next);
      setCatalog(next);
    };

    return (
      <CodexFormFields
        providerId="codex-thirdparty"
        providerName={options.providerName}
        codexApiKey={options.apiKey ?? "sk-test"}
        onApiKeyChange={vi.fn()}
        category="custom"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        partnerPromotionKey={options.partnerPromotionKey}
        planAccessKeyId={options.planAccessKeyId}
        planSecretAccessKey={options.planSecretAccessKey}
        shouldShowSpeedTest={options.shouldShowSpeedTest ?? false}
        codexBaseUrl={options.baseUrl ?? "https://api.thirdparty.example/v1"}
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        takeoverEnabled={options.takeoverEnabled ?? true}
        onTakeoverEnabledChange={vi.fn()}
        apiFormat="openai_chat"
        onApiFormatChange={onApiFormatChange}
        catalogModels={catalog}
        onCatalogModelsChange={handleCatalogChange}
        spawnAgentModels={[]}
        onSpawnAgentModelsChange={vi.fn()}
        codexRouting={{ enabled: false, defaultRouteId: "", routes: [] }}
        onProviderSplitSuggestionChange={
          options.onProviderSplitSuggestionChange
        }
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    );
  }

  return {
    ...render(<Harness />),
    onCatalogChange,
    onApiFormatChange,
    latestCatalog: () => latestCatalog,
  };
}

function renderAutoSplitHarness() {
  const onCatalogChange = vi.fn();
  const onRoutingChange = vi.fn();
  const onTakeoverEnabledChange = vi.fn();
  const onApiFormatChange = vi.fn();
  const onProviderSplitSuggestionChange = vi.fn();
  let latestRouting: CodexRoutingConfig = {
    enabled: false,
    defaultRouteId: "",
    routes: [],
  };

  function Harness() {
    const [catalog, setCatalog] = useState<CodexCatalogModel[]>([]);
    const [routing, setRouting] = useState<CodexRoutingConfig>(latestRouting);

    /// 测试壳同时接住 catalog 和 routing 回写，模拟第一次配置 provider 时的受控状态。
    const handleCatalogChange = (next: CodexCatalogModel[]) => {
      onCatalogChange(next);
      setCatalog(next);
    };
    const handleRoutingChange = (next: CodexRoutingConfig) => {
      latestRouting = next;
      onRoutingChange(next);
      setRouting(next);
    };

    return (
      <CodexFormFields
        providerId="relay-provider"
        providerName="Relay"
        codexApiKey="sk-relay"
        onApiKeyChange={vi.fn()}
        category="custom"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest={false}
        codexBaseUrl="https://relay.example/v1"
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        takeoverEnabled={true}
        onTakeoverEnabledChange={onTakeoverEnabledChange}
        apiFormat="openai_chat"
        onApiFormatChange={onApiFormatChange}
        catalogModels={catalog}
        onCatalogModelsChange={handleCatalogChange}
        spawnAgentModels={[]}
        onSpawnAgentModelsChange={vi.fn()}
        codexRouting={routing}
        onCodexRoutingChange={handleRoutingChange}
        onProviderSplitSuggestionChange={onProviderSplitSuggestionChange}
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    );
  }

  return {
    ...render(<Harness />),
    latestRouting: () => latestRouting,
    onCatalogChange,
    onRoutingChange,
    onTakeoverEnabledChange,
    onApiFormatChange,
    onProviderSplitSuggestionChange,
  };
}

describe("CodexFormFields local model routing", () => {
  it("classifies fetched relay models into Responses and Chat groups", () => {
    expect(
      splitFetchedModelsByLikelyCodexProtocol([
        { id: "openai/gpt-5.5", ownedBy: null },
        { id: "gpt-5.4-mini", ownedBy: null },
        { id: "qwen3.6", ownedBy: null },
        { id: "deepseek-v4-flash", ownedBy: null },
      ]),
    ).toEqual({
      responses: ["openai/gpt-5.5", "gpt-5.4-mini"],
      chat: ["qwen3.6", "deepseek-v4-flash"],
    });
  });

  it("builds split provider suggestion with -responses and -chat model groups", () => {
    const split = buildSplitCodexProviderSuggestionForFetchedModels({
      providerName: "Relay",
      models: [
        { id: "gpt-5.5", ownedBy: null },
        { id: "qwen3.6", ownedBy: null },
      ],
    });

    expect(split).toMatchObject({
      providerName: "Relay",
      responsesModels: ["gpt-5.5"],
      chatModels: ["qwen3.6"],
    });
  });

  it("prompts before preparing split providers after fetching mixed relay models", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: null, contextWindow: 272000 },
      { id: "qwen3.6", ownedBy: null, contextWindow: 128000 },
    ]);
    const {
      latestRouting,
      onRoutingChange,
      onTakeoverEnabledChange,
      onApiFormatChange,
      onProviderSplitSuggestionChange,
    } = renderAutoSplitHarness();

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    expect(await screen.findByText("检测到混合协议模型")).toBeInTheDocument();
    expect(screen.getByText("Relay-responses")).toBeInTheDocument();
    expect(screen.getByText("Relay-chat")).toBeInTheDocument();
    expect(onRoutingChange).not.toHaveBeenCalled();
    expect(latestRouting().routes).toHaveLength(0);

    fireEvent.click(
      screen.getByRole("button", { name: "确认生成两个 provider" }),
    );

    await waitFor(() => {
      expect(onProviderSplitSuggestionChange).toHaveBeenCalledWith(
        expect.objectContaining({
          providerName: "Relay",
          responsesModels: ["gpt-5.5"],
          chatModels: ["qwen3.6"],
        }),
      );
    });
    expect(onRoutingChange).not.toHaveBeenCalled();
    expect(latestRouting().routes).toHaveLength(0);
    expect(onTakeoverEnabledChange).toHaveBeenCalledWith(true);
    expect(onApiFormatChange).not.toHaveBeenCalled();
  });

  it("keeps routing and provider split untouched when mixed relay split prompt is cancelled", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: null, contextWindow: 272000 },
      { id: "qwen3.6", ownedBy: null, contextWindow: 128000 },
    ]);
    const {
      latestRouting,
      onRoutingChange,
      onTakeoverEnabledChange,
      onApiFormatChange,
      onProviderSplitSuggestionChange,
    } = renderAutoSplitHarness();

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    expect(await screen.findByText("检测到混合协议模型")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "暂不拆分" }));

    await waitFor(() => {
      expect(screen.queryByText("检测到混合协议模型")).not.toBeInTheDocument();
    });
    expect(onRoutingChange).not.toHaveBeenCalled();
    expect(onProviderSplitSuggestionChange).toHaveBeenCalledWith(null);
    expect(latestRouting().routes).toHaveLength(0);
    expect(onTakeoverEnabledChange).not.toHaveBeenCalled();
    expect(onApiFormatChange).not.toHaveBeenCalled();
  });

  it("keeps the previous model as upstream when the visible catalog model is renamed", async () => {
    const { latestCatalog } = renderCatalogHarness([
      { model: "gpt-5.5", displayName: "Third-party GPT" },
    ]);

    fireEvent.change(screen.getByLabelText("候选模型名"), {
      target: { value: "gpt-5.5-thirdparty" },
    });

    await waitFor(() => {
      expect(latestCatalog()).toMatchObject([
        {
          model: "gpt-5.5-thirdparty",
          upstreamModel: "gpt-5.5",
        },
      ]);
    });
  });

  it("confirms protocol probing and switches a single provider to Responses when Responses works", async () => {
    vi.mocked(probeCodexResponsesForConfig).mockResolvedValueOnce({
      ok: true,
      status: 200,
      url: "https://api.thirdparty.example/v1/responses",
      model: "gpt-5.5",
      detail: "ok",
    });
    vi.mocked(probeCodexChatForConfig).mockResolvedValueOnce({
      ok: false,
      status: 404,
      url: "https://api.thirdparty.example/v1/chat/completions",
      model: "gpt-5.5",
      detail: "HTTP 404",
    });
    const { onApiFormatChange } = renderCatalogHarness(
      [{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }],
      { shouldShowSpeedTest: true },
    );

    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    );
    expect(screen.getByText("确认测试 Chat / Responses")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() => {
      expect(onApiFormatChange).toHaveBeenCalledWith("openai_responses");
    });
    expect(probeCodexResponsesForConfig).toHaveBeenCalledWith(
      "https://api.thirdparty.example/v1",
      "sk-test",
      "gpt-5.5",
      false,
      "",
    );
    expect(probeCodexChatForConfig).toHaveBeenCalledWith(
      "https://api.thirdparty.example/v1",
      "sk-test",
      "gpt-5.5",
      false,
      "",
    );
  });

  it("shows per-model protocol tags and suggests split providers for mixed probe results", async () => {
    vi.mocked(probeCodexResponsesForConfig)
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        url: "https://api.thirdparty.example/v1/responses",
        model: "gpt-5.5",
        detail: "ok",
      })
      .mockResolvedValueOnce({
        ok: false,
        status: 404,
        url: "https://api.thirdparty.example/v1/responses",
        model: "qwen3.6",
        detail: "HTTP 404",
      })
      .mockResolvedValueOnce({
        ok: false,
        status: 404,
        url: "https://api.thirdparty.example/v1/responses",
        model: "glm-4.5",
        detail: "HTTP 404",
      });
    vi.mocked(probeCodexChatForConfig)
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        url: "https://api.thirdparty.example/v1/chat/completions",
        model: "gpt-5.5",
        detail: "ok",
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        url: "https://api.thirdparty.example/v1/chat/completions",
        model: "qwen3.6",
        detail: "ok",
      })
      .mockResolvedValueOnce({
        ok: false,
        status: 404,
        url: "https://api.thirdparty.example/v1/chat/completions",
        model: "glm-4.5",
        detail: "HTTP 404",
      });
    const onProviderSplitSuggestionChange = vi.fn();
    const { onApiFormatChange } = renderCatalogHarness(
      [
        { model: "gpt-5.5", upstreamModel: "gpt-5.5" },
        { model: "qwen3.6", upstreamModel: "qwen3.6" },
        { model: "glm-4.5", upstreamModel: "glm-4.5" },
      ],
      {
        providerName: "Relay",
        shouldShowSpeedTest: true,
        onProviderSplitSuggestionChange,
      },
    );

    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() => {
      expect(screen.getByTitle("Responses=ok; Chat=ok")).toHaveTextContent(
        "双协议",
      );
    });
    expect(onApiFormatChange).not.toHaveBeenCalled();
    expect(screen.getByTitle("Responses=ok; Chat=ok")).toHaveTextContent(
      "双协议",
    );
    expect(screen.getByTitle("Responses=HTTP 404; Chat=ok")).toHaveTextContent(
      "Chat",
    );
    expect(
      screen.getByTitle("Responses=HTTP 404; Chat=HTTP 404"),
    ).toHaveTextContent("不可用");
    expect(document.body).toHaveTextContent("双协议通过：gpt-5.5");
    expect(document.body).toHaveTextContent("仅 Chat 通过：qwen3.6");
    expect(document.body).toHaveTextContent("双协议失败：glm-4.5");
    expect(await screen.findByText("检测到混合协议模型")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "确认生成两个 provider" }),
    );
    expect(onProviderSplitSuggestionChange).toHaveBeenCalledWith({
      providerName: "Relay",
      responsesModels: ["gpt-5.5"],
      chatModels: ["qwen3.6"],
    });
  });

  it("runs protocol probing with a bounded model concurrency pool", async () => {
    const responseResolvers = new Map<
      string,
      (value: ReturnType<typeof createProbeResult>) => void
    >();
    const chatResolvers = new Map<
      string,
      (value: ReturnType<typeof createProbeResult>) => void
    >();

    vi.mocked(probeCodexResponsesForConfig).mockImplementation(
      async (_baseUrl, _apiKey, model) =>
        new Promise((resolve) => {
          responseResolvers.set(model, resolve);
        }),
    );
    vi.mocked(probeCodexChatForConfig).mockImplementation(
      async (_baseUrl, _apiKey, model) =>
        new Promise((resolve) => {
          chatResolvers.set(model, resolve);
        }),
    );
    const { onApiFormatChange } = renderCatalogHarness(
      [
        { model: "model-a", upstreamModel: "model-a" },
        { model: "model-b", upstreamModel: "model-b" },
        { model: "model-c", upstreamModel: "model-c" },
        { model: "model-d", upstreamModel: "model-d" },
      ],
      { shouldShowSpeedTest: true },
    );

    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() => {
      expect(probeCodexResponsesForConfig).toHaveBeenCalledTimes(3);
      expect(probeCodexChatForConfig).toHaveBeenCalledTimes(3);
    });
    expect(probeCodexResponsesForConfig).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.anything(),
      "model-d",
      expect.anything(),
      expect.anything(),
    );

    responseResolvers.get("model-a")?.(createProbeResult("model-a"));
    chatResolvers.get("model-a")?.(createProbeResult("model-a"));

    await waitFor(() => {
      expect(probeCodexResponsesForConfig).toHaveBeenCalledTimes(4);
      expect(probeCodexChatForConfig).toHaveBeenCalledTimes(4);
    });

    for (const model of ["model-b", "model-c", "model-d"]) {
      responseResolvers.get(model)?.(createProbeResult(model));
      chatResolvers.get(model)?.(createProbeResult(model));
    }

    await waitFor(() => {
      expect(screen.getAllByText("双协议")).toHaveLength(4);
    });
    expect(onApiFormatChange).not.toHaveBeenCalled();
    expect(await screen.findAllByText("双协议")).toHaveLength(4);
  });

  it("opens the protocol probe confirmation above the full screen provider panel", () => {
    renderCatalogHarness([{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }], {
      shouldShowSpeedTest: true,
    });

    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    );

    expect(
      screen.getByText("已打开测试确认框；如果没有看到弹窗，请按 Esc 后重试。"),
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toHaveClass("z-[200]");
    expect(screen.getByText("确认测试 Chat / Responses")).toBeInTheDocument();
  });

  it("keeps catalog fetch and edit controls available when Codex menu mapping is off", async () => {
    renderCatalogHarness([], {
      shouldShowSpeedTest: true,
      takeoverEnabled: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

    const fetchButton = screen.getByRole("button", {
      name: "providerForm.fetchModels",
    });
    const probeButton = screen.getByRole("button", {
      name: "测试 Chat / Responses",
    });

    expect(fetchButton).toBeVisible();
    expect(probeButton).toBeVisible();
    expect(fetchButton.compareDocumentPosition(probeButton)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(screen.getByText("在 Codex /model 菜单中显示")).toBeInTheDocument();
    expect(screen.getByText("模型目录明细")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加模型" }));

    await waitFor(() => {
      expect(screen.getByLabelText("候选模型名")).toBeInTheDocument();
      expect(screen.getByLabelText("上下文窗口")).toBeInTheDocument();
    });
  });

  it("fetches and saves catalog context while Codex menu mapping is off", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: null, contextWindow: 272000 },
    ]);
    const { latestCatalog } = renderCatalogHarness([], {
      shouldShowSpeedTest: true,
      takeoverEnabled: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => {
      expect(latestCatalog()).toEqual([
        {
          model: "gpt-5.5",
          displayName: "gpt-5.5",
          upstreamModel: "gpt-5.5",
          contextWindow: "272000",
        },
      ]);
    });
  });

  it("points users to fetch models when protocol probing has no catalog", async () => {
    renderCatalogHarness([], { shouldShowSpeedTest: true });

    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "请先在上方“模型目录与上下文”点击“获取模型列表”，或手动添加模型后再测试。",
    );
    const fetchButton = screen.getByRole("button", {
      name: "providerForm.fetchModels",
    });
    expect(fetchButton).toHaveClass("border-blue-500");
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    expect(probeCodexResponsesForConfig).not.toHaveBeenCalled();
    expect(probeCodexChatForConfig).not.toHaveBeenCalled();
  });

  it("surfaces protocol probe exceptions inline instead of looking frozen", async () => {
    vi.mocked(probeCodexResponsesForConfig).mockRejectedValueOnce(
      new Error("backend timeout"),
    );
    renderCatalogHarness([{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }], {
      shouldShowSpeedTest: true,
    });

    fireEvent.click(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "协议测试中断：backend timeout",
    );
    expect(
      screen.getByRole("button", { name: "测试 Chat / Responses" }),
    ).toBeEnabled();
  });

  it("merges fetched models by upstream model without overwriting a visible alias", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: null, contextWindow: 272000 },
    ]);
    const { latestCatalog } = renderCatalogHarness([
      {
        model: "gpt-5.5-thirdparty",
        upstreamModel: "gpt-5.5",
        displayName: "Third-party GPT",
      },
    ]);

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => {
      expect(latestCatalog()).toEqual([
        {
          model: "gpt-5.5-thirdparty",
          upstreamModel: "gpt-5.5",
          displayName: "Third-party GPT",
          contextWindow: "272000",
        },
      ]);
    });
  });

  it("falls back to data-plane models when AgentPlan AK/SK is missing but API Key exists", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "ark-code-latest", ownedBy: "volcengine" },
      { id: "doubao-seed-1.6", ownedBy: "volcengine" },
    ]);
    const { latestCatalog } = renderCatalogHarness(
      [{ model: "ark-code-latest", upstreamModel: "ark-code-latest" }],
      {
        providerName: "火山Agentplan",
        partnerPromotionKey: "volcengine_agentplan",
        baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      },
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => {
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://ark.cn-beijing.volces.com/api/coding/v3",
        "sk-test",
        false,
        undefined,
        "",
        undefined,
      );
      expect(latestCatalog().map((model) => model.model)).toEqual([
        "ark-code-latest",
        "doubao-seed-1.6",
      ]);
    });
  });

  it("keeps AgentPlan catalog when both inference key and AK/SK are missing", () => {
    const { latestCatalog } = renderCatalogHarness(
      [{ model: "ark-code-latest", upstreamModel: "ark-code-latest" }],
      {
        providerName: "火山Agentplan",
        partnerPromotionKey: "volcengine_agentplan",
        baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
        apiKey: "",
      },
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    expect(fetchModelsForConfig).not.toHaveBeenCalled();
    expect(latestCatalog()).toEqual([
      { model: "ark-code-latest", upstreamModel: "ark-code-latest" },
    ]);
  });

  it("fetches AgentPlan models through Volcengine OpenAPI when AK/SK credentials exist", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "doubao-seed-1.6", ownedBy: "volcengine" },
    ]);
    const { latestCatalog } = renderCatalogHarness(
      [{ model: "ark-code-latest", upstreamModel: "ark-code-latest" }],
      {
        providerName: "火山Agentplan",
        partnerPromotionKey: "volcengine_agentplan",
        baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
        planAccessKeyId: "AKLTtest",
        planSecretAccessKey: "secret",
      },
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => {
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://ark.cn-beijing.volces.com/api/coding/v3",
        "sk-test",
        false,
        undefined,
        "",
        {
          action: "ListArkAgentPlanModel",
          accessKeyId: "AKLTtest",
          secretAccessKey: "secret",
        },
      );
      expect(latestCatalog().map((model) => model.model)).toEqual([
        "ark-code-latest",
        "doubao-seed-1.6",
      ]);
    });
  });

  it("uses model mapping checkboxes and arrows for catalog retention and order", async () => {
    const { latestCatalog } = renderCatalogHarness([
      { model: "model-a", upstreamModel: "model-a" },
      { model: "model-b", upstreamModel: "model-b" },
      { model: "model-c", upstreamModel: "model-c" },
    ]);

    fireEvent.click(screen.getByLabelText("保留 model-b"));

    await waitFor(() => {
      expect(latestCatalog().map((model) => model.model)).toEqual([
        "model-a",
        "model-c",
      ]);
    });

    fireEvent.click(screen.getAllByTitle("上移")[1]);

    await waitFor(() => {
      expect(latestCatalog().map((model) => model.model)).toEqual([
        "model-c",
        "model-a",
      ]);
    });
  });

  it("shows local model routing even when endpoint speed tools are hidden", () => {
    renderRoutingHarness(
      { enabled: false, defaultRouteId: "", routes: [] },
      { shouldShowSpeedTest: false },
    );

    expect(screen.getByText("Codex 多模型路由")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "添加路由" }),
    ).toBeInTheDocument();
  });

  it("adds and edits a route through the dialog without persisting rowId", async () => {
    const { latestRouting } = renderRoutingHarness();

    fireEvent.click(screen.getByRole("button", { name: "添加路由" }));

    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      expect(latestRouting().routes).toHaveLength(1);
    });

    fireEvent.change(screen.getByPlaceholderText("路由 ID"), {
      target: { value: "deepseek" },
    });
    fireEvent.change(screen.getByPlaceholderText("路由名称"), {
      target: { value: "DeepSeek" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("匹配模型，多个用英文逗号分隔"),
      {
        target: { value: "deepseek-v4-flash, deepseek-v4-pro" },
      },
    );
    fireEvent.change(
      screen.getByPlaceholderText("匹配前缀，多个用英文逗号分隔"),
      {
        target: { value: "deepseek-" },
      },
    );
    fireEvent.change(screen.getByPlaceholderText("上游 Base URL"), {
      target: { value: "https://api.deepseek.example" },
    });
    fireEvent.change(screen.getByPlaceholderText("路由 API Key"), {
      target: { value: "sk-route" },
    });
    fireEvent.change(screen.getByPlaceholderText("codex模型=上游模型"), {
      target: { value: "deepseek-v4-flash=deepseek-chat" },
    });

    await waitFor(() => {
      expect(latestRouting().routes?.[0]).toMatchObject({
        id: "deepseek",
        label: "DeepSeek",
        match: {
          models: ["deepseek-v4-flash", "deepseek-v4-pro"],
          prefixes: ["deepseek-"],
        },
        upstream: {
          baseUrl: "https://api.deepseek.example",
          apiKey: "sk-route",
          modelMap: { "deepseek-v4-flash": "deepseek-chat" },
        },
      });
    });
    expect(latestRouting().routes?.[0]).not.toHaveProperty("rowId");
  });

  it("removes a route from the list and writes the shortened routing config", async () => {
    const { latestRouting, container } = renderRoutingHarness({
      enabled: true,
      defaultRouteId: "",
      routes: [
        {
          id: "deepseek",
          label: "DeepSeek",
          enabled: true,
          match: { models: ["deepseek-v4-flash"], prefixes: [] },
          upstream: {
            baseUrl: "https://api.deepseek.example",
            apiFormat: "openai_chat",
            auth: { source: "provider_config" },
          },
          capabilities: { textOnly: true, inputModalities: ["text"] },
        },
      ],
    });

    const deleteButton = container.querySelector('button[title="删除"]');
    expect(deleteButton).not.toBeNull();
    fireEvent.click(deleteButton!);

    await waitFor(() => {
      expect(latestRouting().routes).toEqual([]);
    });
  });
});
