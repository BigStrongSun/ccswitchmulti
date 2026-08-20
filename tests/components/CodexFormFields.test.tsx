import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexRoutingConfig,
} from "@/types";
import type {
  CodexModelReasoningResolution,
  CodexReasoningDiscoveryOutcome,
} from "@/types/codexSubagentV2";

const reasoningApiMocks = vi.hoisted(() => ({
  resolve: vi.fn(),
  trigger: vi.fn(),
}));

vi.mock("@/lib/api/codexSubagentV2", () => ({
  codexSubagentV2Api: {
    resolveModelReasoningCapability: reasoningApiMocks.resolve,
    triggerModelReasoningDetection: reasoningApiMocks.trigger,
  },
}));

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
  reasoningApiMocks.resolve.mockReset();
  reasoningApiMocks.trigger.mockReset();
  reasoningApiMocks.resolve.mockImplementation(
    async (_settings, _provider, model) =>
      createUnknownReasoningResolution(model),
  );
});

function createUnknownReasoningResolution(
  model: string,
): CodexModelReasoningResolution {
  return {
    model,
    capability: null,
    source: "unknown",
    fingerprint: "",
    resolved: {
      supportKind: "unknown",
      confidence: "unverified",
      codexSelectableEfforts: [],
      providerAcceptedEfforts: [],
      providerDefaultEffort: null,
      disableAllowed: false,
      effortMap: {},
    },
    hasDetectionCandidate: false,
    detection: null,
  };
}

function createDetectedReasoningOutcome(
  model: string,
): Extract<CodexReasoningDiscoveryOutcome, { found: unknown }> {
  return {
    found: {
      providerKey: "codex-thirdparty",
      model,
      fetchedAt: 1_700_000_000_000,
      source: "openrouter_api",
      reasoning: {
        supportedEfforts: ["low", "high"],
        defaultEffort: "high",
        mandatory: false,
        defaultEnabled: true,
      },
    },
  };
}

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

function createProbeFailure(model: string, detail = "HTTP 401") {
  return {
    ok: false,
    status: 401,
    url: "https://api.thirdparty.example/v1/probe",
    model,
    detail,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
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
    allowModelMenuProjectionToggle?: boolean;
    openAdvancedOptions?: boolean;
    presetCatalogModels?: CodexCatalogModel[];
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
        allowModelMenuProjectionToggle={
          options.allowModelMenuProjectionToggle ?? true
        }
        apiFormat="openai_chat"
        onApiFormatChange={onApiFormatChange}
        catalogModels={catalog}
        presetCatalogModels={options.presetCatalogModels}
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

  const renderResult = render(<Harness />);
  if (options.openAdvancedOptions ?? true) {
    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));
  }

  return {
    ...renderResult,
    onCatalogChange,
    onApiFormatChange,
    latestCatalog: () => latestCatalog,
  };
}

interface ReadinessIdentityState {
  baseUrl: string;
  apiKey: string;
  selectedAccountId: string | null;
  customUserAgent: string;
  headersOverride: string;
  apiFormat: CodexApiFormat;
  defaultModel: string;
  catalog: CodexCatalogModel[];
}

function renderReadinessIdentityHarness(
  overrides: Partial<ReadinessIdentityState> = {},
) {
  const initial: ReadinessIdentityState = {
    baseUrl: "https://old.example/v1",
    apiKey: "sk-old",
    selectedAccountId: "account-old",
    customUserAgent: "ccswitch-old",
    headersOverride: '{"X-Route":"old"}',
    apiFormat: "openai_chat",
    defaultModel: "model-a",
    catalog: [
      {
        model: "model-a",
        upstreamModel: "model-a",
        inputModalities: ["text"],
        supportsImage: false,
      },
    ],
    ...overrides,
  };
  const onApiFormatChange = vi.fn();
  let patchIdentity: (patch: Partial<ReadinessIdentityState>) => void = () => {
    throw new Error("readiness identity harness is not mounted");
  };

  function Harness() {
    const [identity, setIdentity] = useState(initial);
    patchIdentity = (patch) =>
      setIdentity((current) => ({ ...current, ...patch }));

    return (
      <CodexFormFields
        providerId="identity-provider"
        providerName="Identity Provider"
        selectedXaiAccountId={identity.selectedAccountId}
        onXaiAccountSelect={vi.fn()}
        codexApiKey={identity.apiKey}
        onApiKeyChange={(apiKey) => patchIdentity({ apiKey })}
        category="custom"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest
        codexBaseUrl={identity.baseUrl}
        onBaseUrlChange={(baseUrl) => patchIdentity({ baseUrl })}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        takeoverEnabled
        onTakeoverEnabledChange={vi.fn()}
        codexModel={identity.defaultModel}
        onModelChange={(defaultModel) => patchIdentity({ defaultModel })}
        apiFormat={identity.apiFormat}
        onApiFormatChange={(apiFormat) => {
          onApiFormatChange(apiFormat);
          patchIdentity({ apiFormat });
        }}
        catalogModels={identity.catalog}
        onCatalogModelsChange={(catalog) => patchIdentity({ catalog })}
        spawnAgentModels={[]}
        onSpawnAgentModelsChange={vi.fn()}
        codexRouting={{ enabled: false, defaultRouteId: "", routes: [] }}
        speedTestEndpoints={[]}
        customUserAgent={identity.customUserAgent}
        onCustomUserAgentChange={(customUserAgent) =>
          patchIdentity({ customUserAgent })
        }
        localProxyHeadersOverride={identity.headersOverride}
        onLocalProxyHeadersOverrideChange={(headersOverride) =>
          patchIdentity({ headersOverride })
        }
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    );
  }

  const renderResult = render(<Harness />);
  return {
    ...renderResult,
    onApiFormatChange,
    updateIdentity(patch: Partial<ReadinessIdentityState>) {
      act(() => patchIdentity(patch));
    },
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

  const renderResult = render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

  return {
    ...renderResult,
    latestRouting: () => latestRouting,
    onCatalogChange,
    onRoutingChange,
    onTakeoverEnabledChange,
    onApiFormatChange,
    onProviderSplitSuggestionChange,
  };
}

describe("CodexFormFields local model routing", () => {
  it("shows model reasoning as a standalone section before advanced catalog details", () => {
    renderCatalogHarness([{ model: "qwen3.8" }]);

    expect(
      screen.getByRole("heading", { name: "模型推理能力" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("qwen3.8推理能力来源")).toBeInTheDocument();
    expect(screen.getByText("模型目录明细")).toBeInTheDocument();
    expect(screen.queryByText(/Codex 推理能力（/)).not.toBeInTheDocument();
    expect(
      screen
        .getByRole("heading", { name: "模型推理能力" })
        .compareDocumentPosition(screen.getByText("模型目录明细")) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("renders the resolved reasoning card and lets the user declare an unknown model", async () => {
    renderCatalogHarness([{ model: "qwen3.8" }]);
    await waitFor(() => expect(reasoningApiMocks.resolve).toHaveBeenCalled());

    expect(
      await screen.findByText("未知（使用服务端默认）"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("使用服务端默认（不发送推理参数）。"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "手动声明" }));

    await waitFor(() => {
      expect(screen.getByLabelText("qwen3.8推理能力 JSON")).not.toHaveValue("");
    });
  });

  it("adopts a detected reasoning snapshot into the catalog declaration", async () => {
    const detected = createDetectedReasoningOutcome("qwen3.8");
    reasoningApiMocks.resolve.mockImplementation(
      async (_settings, _provider, model) => ({
        ...createUnknownReasoningResolution(model),
        hasDetectionCandidate: true,
        detection: detected.found,
      }),
    );
    const { latestCatalog } = renderCatalogHarness([{ model: "qwen3.8" }]);

    expect(await screen.findByText("采用检测结果")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "采用检测结果" }));

    await waitFor(() => {
      expect(latestCatalog()[0].reasoning).toEqual(
        expect.objectContaining({
          source: "user",
          supportedEfforts: ["low", "high"],
          defaultEffort: "high",
          disableAllowed: true,
        }),
      );
    });
  });

  it("re-detects a model through the read-only capability command", async () => {
    const detected = createDetectedReasoningOutcome("qwen3.8");
    let redetected = false;
    reasoningApiMocks.resolve.mockImplementation(
      async (_settings, _provider, model) =>
        redetected
          ? {
              ...createUnknownReasoningResolution(model),
              source: "detection",
              capability: {
                schemaVersion: 2,
                supportStatus: "confirmed_supported",
                controlKind: "graded",
                supportedEfforts: ["low", "high"],
                defaultEffort: "high",
                disableAllowed: true,
                upstream: {
                  format: "reasoning_object",
                  parameter: "reasoning.effort",
                },
              },
              resolved: {
                supportKind: "effort_levels",
                confidence: "confirmed",
                codexSelectableEfforts: ["low", "high"],
                providerAcceptedEfforts: ["low", "high"],
                providerDefaultEffort: "high",
                disableAllowed: true,
                effortMap: { low: "low", high: "high" },
              },
              hasDetectionCandidate: true,
              detection: detected.found,
            }
          : createUnknownReasoningResolution(model),
    );
    reasoningApiMocks.trigger.mockImplementationOnce(async () => {
      redetected = true;
      return detected;
    });
    renderCatalogHarness([{ model: "qwen3.8" }]);

    expect(
      await screen.findByRole("button", { name: "重新检测" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重新检测" }));

    await waitFor(() => {
      expect(reasoningApiMocks.trigger).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "codex-thirdparty",
          settingsConfig: { base_url: "https://api.thirdparty.example/v1" },
        }),
        "qwen3.8",
      );
    });
    expect(await screen.findByText("支持推理")).toBeInTheDocument();
  });

  it("keeps built-in reasoning read-only until an explicit override and restores the preset", async () => {
    const builtinReasoning: NonNullable<CodexCatalogModel["reasoning"]> = {
      supported: true as const,
      supportedEfforts: ["low", "high", "max"],
      defaultEffort: "high" as const,
      disableAllowed: false,
      upstream: {
        format: "reasoning_object" as const,
        parameter: "reasoning.effort" as const,
      },
      source: "builtin" as const,
    };
    const { latestCatalog } = renderCatalogHarness(
      [{ model: "deepseek-v4-pro", reasoning: builtinReasoning }],
      {
        presetCatalogModels: [
          { model: "deepseek-v4-pro", reasoning: builtinReasoning },
        ],
      },
    );

    const textarea = screen.getByLabelText("deepseek-v4-pro推理能力 JSON");
    expect(textarea).toHaveAttribute("readonly");
    expect(screen.getByText(/内置预设，只读/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "创建高级覆盖" }));
    await waitFor(() => {
      expect(latestCatalog()[0].reasoning?.source).toBe("user");
    });
    expect(
      screen.getByLabelText("deepseek-v4-pro推理能力 JSON"),
    ).not.toHaveAttribute("readonly");
    expect(screen.getByText(/已偏离内置预设/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "恢复内置默认" }));
    await waitFor(() => {
      expect(latestCatalog()[0].reasoning).toEqual(builtinReasoning);
    });
    expect(
      screen.getByLabelText("deepseek-v4-pro推理能力 JSON"),
    ).toHaveAttribute("readonly");
  });
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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
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

  it("invalidates successful readiness for every routing and catalog identity input", async () => {
    vi.mocked(probeCodexResponsesForConfig).mockImplementation(
      async (_baseUrl, _apiKey, model) => createProbeResult(model),
    );
    vi.mocked(probeCodexChatForConfig).mockImplementation(
      async (_baseUrl, _apiKey, model) => createProbeResult(model),
    );
    const { updateIdentity } = renderReadinessIdentityHarness();

    const validateCurrentIdentity = async () => {
      fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
      fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
      expect(await screen.findByText("可加入 MultiRouter")).toBeInTheDocument();
    };
    const expectInvalidated = async () => {
      await waitFor(() => {
        expect(
          screen.queryByText("可加入 MultiRouter"),
        ).not.toBeInTheDocument();
      });
      expect(screen.getByText("建议先验证连接")).toBeInTheDocument();
    };

    await validateCurrentIdentity();
    const identityChanges: Array<Partial<ReadinessIdentityState>> = [
      { baseUrl: "https://new.example/v1" },
      { apiKey: "sk-new" },
      { selectedAccountId: "account-new" },
      { customUserAgent: "ccswitch-new" },
      { headersOverride: '{"X-Route":"new"}' },
      { apiFormat: "openai_responses" },
      { defaultModel: "model-a-alias" },
      {
        catalog: [
          {
            model: "model-a",
            upstreamModel: "model-a",
            inputModalities: ["text", "image"],
            supportsImage: true,
          },
        ],
      },
    ];

    for (const patch of identityChanges) {
      updateIdentity(patch);
      await expectInvalidated();
      await validateCurrentIdentity();
    }
  });

  it("ignores an older probe completion after a newer provider identity succeeds", async () => {
    type ProbeResult = Awaited<ReturnType<typeof probeCodexResponsesForConfig>>;
    const oldResponses = deferred<ProbeResult>();
    const oldChat = deferred<ProbeResult>();
    const newResponses = deferred<ProbeResult>();
    const newChat = deferred<ProbeResult>();

    vi.mocked(probeCodexResponsesForConfig).mockImplementation(
      async (baseUrl) =>
        baseUrl.includes("old") ? oldResponses.promise : newResponses.promise,
    );
    vi.mocked(probeCodexChatForConfig).mockImplementation(async (baseUrl) =>
      baseUrl.includes("old") ? oldChat.promise : newChat.promise,
    );
    const { updateIdentity } = renderReadinessIdentityHarness();

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    await waitFor(() => {
      expect(probeCodexResponsesForConfig).toHaveBeenCalledWith(
        "https://old.example/v1",
        expect.anything(),
        "model-a",
        expect.anything(),
        expect.anything(),
      );
    });

    updateIdentity({ baseUrl: "https://new.example/v1" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "验证连接" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await act(async () => {
      newResponses.resolve(createProbeResult("model-a", "new responses ok"));
      newChat.resolve(createProbeResult("model-a", "new chat ok"));
      await Promise.resolve();
    });
    expect(await screen.findByText("可加入 MultiRouter")).toBeInTheDocument();

    await act(async () => {
      oldResponses.resolve(createProbeFailure("model-a", "old credentials"));
      oldChat.resolve(createProbeFailure("model-a", "old credentials"));
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText("可加入 MultiRouter")).toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });
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

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
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

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
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
      openAdvancedOptions: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));
    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));

    expect(
      screen.getByText("已打开验证确认框；如果没有看到弹窗，请按 Esc 后重试。"),
    ).toBeVisible();
    expect(screen.getByRole("dialog")).toHaveClass("z-[200]");
    expect(screen.getByText("确认测试 Chat / Responses")).toBeInTheDocument();
  });

  it("keeps catalog fetch and edit controls available when Codex menu mapping is off", async () => {
    renderCatalogHarness([], {
      shouldShowSpeedTest: true,
      takeoverEnabled: false,
      openAdvancedOptions: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

    const fetchButton = screen.getByRole("button", {
      name: "同步模型",
    });
    const probeButton = screen.getByRole("button", {
      name: "验证连接",
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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

  it("preserves fetched image support and updates existing rows to explicit text-only capabilities", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      {
        id: "vision-model",
        ownedBy: null,
        inputModalities: ["text", "image"],
        supportsImage: true,
      },
      {
        id: "text-model",
        ownedBy: null,
        inputModalities: ["text"],
        supportsImage: false,
      },
    ]);
    const { latestCatalog } = renderCatalogHarness([
      {
        model: "text-model",
        upstreamModel: "text-model",
        displayName: "Existing text model",
        inputModalities: ["text", "image"],
        supportsImage: true,
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

    await waitFor(() => {
      expect(latestCatalog()).toEqual([
        {
          model: "text-model",
          upstreamModel: "text-model",
          displayName: "Existing text model",
          contextWindow: "",
          inputModalities: ["text"],
          supportsImage: false,
        },
        {
          model: "vision-model",
          upstreamModel: "vision-model",
          displayName: "vision-model",
          contextWindow: "",
          inputModalities: ["text", "image"],
          supportsImage: true,
        },
      ]);
    });
  });

  it("points users to fetch models when protocol probing has no catalog", async () => {
    renderCatalogHarness([], { shouldShowSpeedTest: true });

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "请先在“模型与兼容性”同步模型，或在高级设置中手动添加至少一个模型后再验证。",
    );
    const fetchButton = screen.getByRole("button", {
      name: "同步模型",
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

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "协议测试中断：backend timeout",
    );
    expect(screen.getByRole("button", { name: "验证连接" })).toBeEnabled();
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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

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

  it("hides the legacy route editor while retaining the supplied routing state", () => {
    const { latestRouting } = renderRoutingHarness({
      enabled: true,
      defaultRouteId: "deepseek",
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

    expect(screen.queryByText("Codex 多模型路由")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "添加路由" }),
    ).not.toBeInTheDocument();
    expect(latestRouting()).toMatchObject({
      enabled: true,
      defaultRouteId: "deepseek",
      routes: [{ id: "deepseek" }],
    });
  });

  it("places the explained custom menu projection control last in advanced options", () => {
    renderCatalogHarness([], {
      shouldShowSpeedTest: false,
      takeoverEnabled: true,
      openAdvancedOptions: false,
    });

    expect(
      screen.queryByText("在 Codex /model 菜单中显示"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

    const overrides = screen.getByText("本地代理请求覆盖");
    const projection = screen.getByText("在 Codex /model 菜单中显示");
    expect(projection).toBeInTheDocument();
    expect(
      overrides.compareDocumentPosition(projection) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      screen.getByText(/它不控制 Provider、代理或 MultiRouter 是否可用/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/仅当你要使用自己维护的 model_catalog_json 时关闭/),
    ).toBeInTheDocument();
  });

  it("does not offer a menu projection opt-out for maintained presets", () => {
    renderCatalogHarness([], {
      allowModelMenuProjectionToggle: false,
      openAdvancedOptions: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

    expect(
      screen.queryByText("在 Codex /model 菜单中显示"),
    ).not.toBeInTheDocument();
  });
});
