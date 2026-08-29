import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { preflightCodexProviderProtocolCompatibility } from "@/lib/api/protocol-compatibility";
import type {
  CodexProtocolCompatibilityRecord,
  CodexProtocolProbeProgressEvent,
  CodexProtocolTransport,
  CodexProviderProtocolPreflightOutcome,
} from "@/lib/api/protocol-compatibility";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  CodexHistoryReplay,
  CodexProtocolMode,
  CodexReasoningProjection,
  CodexRoutingConfig,
  CodexToolSchemaDialect,
  PromptCacheRoutingMode,
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

vi.mock("@/lib/api/protocol-compatibility", () => ({
  preflightCodexProviderProtocolCompatibility: vi.fn(),
}));

vi.mock("@/components/ui/form", () => ({
  FormLabel: ({ children }: { children: ReactNode }) => (
    <label>{children}</label>
  ),
}));

beforeEach(() => {
  vi.useRealTimers();
  vi.mocked(fetchModelsForConfig).mockReset();
  vi.mocked(preflightCodexProviderProtocolCompatibility).mockReset();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    writable: true,
    value: vi.fn(),
  });
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

function createDeepProbeRecord(
  model: string,
  selectedTransport: CodexProtocolTransport | null,
  readiness: "verified" | "partial" | "unverified" = "verified",
): CodexProtocolCompatibilityRecord {
  const passed = selectedTransport ? "passed" : "failed";
  return {
    probeVersion: 1,
    target: {
      provider_id: "codex-thirdparty",
      route_id: null,
      public_model: model,
      upstream_model: model,
      transport: selectedTransport ?? "open_ai_responses",
      endpoint_fingerprint: "redacted-endpoint",
      authentication_kind: "bearer",
      credential_fingerprint: "redacted-credential",
    },
    result: {
      selected_transport: selectedTransport,
      readiness,
      branches: (["open_ai_responses", "open_ai_chat"] as const).map(
        (transport) => ({
          assessment: {
            transport,
            baseline: transport === selectedTransport ? passed : "failed",
            streaming: transport === selectedTransport ? passed : "skipped",
            forced_tool: transport === selectedTransport ? passed : "skipped",
            continuation: transport === selectedTransport ? passed : "skipped",
          },
          reasoning_shape: {
            semantic:
              transport === selectedTransport
                ? ("readable" as const)
                : ("none" as const),
            source:
              transport === selectedTransport
                ? transport === "open_ai_chat"
                  ? ("reasoning_content" as const)
                  : ("native_responses" as const)
                : ("none" as const),
            pre_tool_visible_content: "absent" as const,
          },
        }),
      ),
    },
    testedAt: 1_700_000_000,
    expiresAt: 1_700_086_400,
  };
}

function createDeepProbeOutcome(
  records: CodexProtocolCompatibilityRecord[],
): CodexProviderProtocolPreflightOutcome {
  return {
    provider: {
      id: "codex-thirdparty",
      name: "Third party",
      settingsConfig: {},
    },
    records,
    receiptIds: records.map(
      (record) => `receipt-${record.target.public_model}`,
    ),
    protocolApplied: records.every(
      (record) =>
        record.result.selected_transport ===
        records[0]?.result.selected_transport,
    ),
  };
}

function emitFinishedProgress(
  records: CodexProtocolCompatibilityRecord[],
  onProgress: (event: CodexProtocolProbeProgressEvent) => void,
) {
  for (const record of records) {
    const model = record.target.public_model;
    onProgress({ kind: "candidate_started", model });
    onProgress({
      kind: "candidate_finished",
      model,
      selectedTransport: record.result.selected_transport,
      readiness: record.result.readiness,
    });
  }
  onProgress({
    kind: "batch_finished",
    total: records.length,
    verified: records.filter((record) => record.result.readiness === "verified")
      .length,
    partial: records.filter((record) => record.result.readiness === "partial")
      .length,
    failed: records.filter((record) => record.result.readiness === "unverified")
      .length,
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

async function waitForReasoningResolution() {
  await act(async () => {
    await Promise.all(
      reasoningApiMocks.resolve.mock.results.map((result) => result.value),
    );
  });
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
    customUserAgent?: string;
    headersOverride?: string;
    bodyOverride?: string;
    isFullUrl?: boolean;
    codexChatReasoning?: CodexChatReasoning;
    promptCacheRouting?: PromptCacheRoutingMode;
    protocolMode?: CodexProtocolMode;
    reasoningProjection?: CodexReasoningProjection;
    toolSchemaDialect?: CodexToolSchemaDialect;
    historyReplay?: CodexHistoryReplay;
  } = {},
) {
  const onCatalogChange = vi.fn();
  const onApiFormatChange = vi.fn();
  const onProtocolProbeReceiptIdsChange = vi.fn();
  let latestCatalog = initialCatalog;

  function Harness() {
    const [catalog, setCatalog] = useState<CodexCatalogModel[]>(initialCatalog);
    const [apiFormat, setApiFormat] = useState<CodexApiFormat>("openai_chat");

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
        isFullUrl={options.isFullUrl ?? false}
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
        apiFormat={apiFormat}
        onApiFormatChange={(nextApiFormat) => {
          onApiFormatChange(nextApiFormat);
          setApiFormat(nextApiFormat);
        }}
        catalogModels={catalog}
        presetCatalogModels={options.presetCatalogModels}
        onCatalogModelsChange={handleCatalogChange}
        spawnAgentModels={[]}
        onSpawnAgentModelsChange={vi.fn()}
        codexRouting={{ enabled: false, defaultRouteId: "", routes: [] }}
        speedTestEndpoints={[]}
        customUserAgent={options.customUserAgent ?? ""}
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride={options.headersOverride ?? ""}
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride={options.bodyOverride ?? ""}
        onLocalProxyBodyOverrideChange={vi.fn()}
        codexChatReasoning={options.codexChatReasoning}
        promptCacheRouting={options.promptCacheRouting}
        protocolMode={options.protocolMode}
        reasoningProjection={options.reasoningProjection}
        toolSchemaDialect={options.toolSchemaDialect}
        historyReplay={options.historyReplay}
        onProtocolProbeReceiptIdsChange={onProtocolProbeReceiptIdsChange}
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
    onProtocolProbeReceiptIdsChange,
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
  let latestRouting: CodexRoutingConfig = {
    enabled: false,
    defaultRouteId: "",
    routes: [],
  };
  let latestCatalog: CodexCatalogModel[] = [];

  function Harness() {
    const [catalog, setCatalog] = useState<CodexCatalogModel[]>([]);
    const [routing, setRouting] = useState<CodexRoutingConfig>(latestRouting);

    /// 测试壳同时接住 catalog 和 routing 回写，模拟第一次配置 provider 时的受控状态。
    const handleCatalogChange = (next: CodexCatalogModel[]) => {
      latestCatalog = next;
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
    latestCatalog: () => latestCatalog,
    onCatalogChange,
    onRoutingChange,
    onTakeoverEnabledChange,
    onApiFormatChange,
  };
}

describe("CodexFormFields local model routing", () => {
  it("keeps the model catalog in normal settings before model reasoning", async () => {
    renderCatalogHarness([{ model: "qwen3.8" }], {
      openAdvancedOptions: false,
    });
    await waitForReasoningResolution();

    expect(screen.getByText("模型目录明细")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "模型推理能力" }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "配置 qwen3.8 的推理能力" }),
    );
    expect(screen.getByLabelText("qwen3.8推理能力来源")).toBeInTheDocument();
    expect(screen.queryByText(/Codex 推理能力（/)).not.toBeInTheDocument();
    expect(
      screen
        .getByText("模型目录明细")
        .compareDocumentPosition(
          screen.getByRole("heading", { name: "模型推理能力" }),
        ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("lets users override and restore a preset model's input capability", async () => {
    const preset: CodexCatalogModel = {
      model: "deepseek-v4-flash-vision-exp",
      inputModalities: ["text", "image"],
      supportsImage: true,
      textOnly: false,
    };
    const { latestCatalog } = renderCatalogHarness([preset], {
      presetCatalogModels: [preset],
    });

    expect(
      screen.getByRole("button", {
        name: "deepseek-v4-flash-vision-exp 文本与图像",
      }),
    ).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(
      screen.getByRole("button", {
        name: "deepseek-v4-flash-vision-exp 仅文本",
      }),
    );
    await waitFor(() => {
      expect(latestCatalog()[0]).toEqual(
        expect.objectContaining({
          inputModalities: ["text"],
          supportsImage: false,
          textOnly: true,
        }),
      );
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "deepseek-v4-flash-vision-exp 恢复 CCSM 输入能力预设",
      }),
    );
    await waitFor(() => {
      expect(latestCatalog()[0]).toEqual(
        expect.objectContaining({
          inputModalities: ["text", "image"],
          supportsImage: true,
          textOnly: false,
        }),
      );
    });
  });

  it("keeps Ultra independent from automatic reasoning discovery", async () => {
    reasoningApiMocks.resolve.mockImplementation(
      async (_settings, _provider, model) => ({
        model,
        source: "library",
        fingerprint: "library-deepseek",
        capability: {
          schemaVersion: 2,
          supportStatus: "confirmed_supported",
          controlKind: "graded",
          supportedEfforts: ["low", "high"],
          defaultEffort: "high",
          disableAllowed: false,
          upstream: {
            format: "string",
            parameter: "reasoning_effort",
            effortMap: { low: "low", high: "high", max: "high" },
          },
          source: "provider",
        },
        resolved: {
          supportKind: "effort_levels",
          confidence: "maintained",
          codexSelectableEfforts: ["low", "high", "max"],
          providerAcceptedEfforts: ["low", "high"],
          providerDefaultEffort: "high",
          disableAllowed: false,
          effortMap: { low: "low", high: "high", max: "high" },
        },
        hasDetectionCandidate: false,
        detection: null,
      }),
    );
    const { latestCatalog } = renderCatalogHarness([
      { model: "deepseek-v4-flash" },
    ]);

    fireEvent.click(
      screen.getByRole("button", {
        name: "配置 deepseek-v4-flash 的推理能力",
      }),
    );

    expect(
      await screen.findByText(/自动发现会按当前 Provider、模型和已验证声明/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: "解锁 deepseek-v4-flash 的 Ultra 档",
      }),
    );
    fireEvent.change(
      screen.getByRole("combobox", {
        name: "deepseek-v4-flash Ultra 对应的 Provider 推理强度",
      }),
      { target: { value: "high" } },
    );

    await waitFor(() => {
      expect(latestCatalog()[0]).toEqual(
        expect.objectContaining({
          codexUltra: { enabled: true, providerEffort: "high" },
        }),
      );
    });
    expect(latestCatalog()[0].reasoning).toBeUndefined();
  });

  it("renders the resolved reasoning card and lets the user declare an unknown model", async () => {
    renderCatalogHarness([{ model: "qwen3.8" }]);
    await waitFor(() => expect(reasoningApiMocks.resolve).toHaveBeenCalled());
    fireEvent.click(
      screen.getByRole("button", { name: "配置 qwen3.8 的推理能力" }),
    );

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
    fireEvent.click(
      screen.getByRole("button", { name: "配置 qwen3.8 的推理能力" }),
    );

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
    fireEvent.click(
      screen.getByRole("button", { name: "配置 qwen3.8 的推理能力" }),
    );

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

    fireEvent.click(
      screen.getByRole("button", {
        name: "配置 deepseek-v4-pro 的推理能力",
      }),
    );
    const textarea = screen.getByLabelText("deepseek-v4-pro推理能力 JSON");
    expect(textarea).toHaveAttribute("readonly");
    expect(screen.getByText("能力来源：CCSM 受维护声明")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "创建高级覆盖" }));
    await waitFor(() => {
      expect(latestCatalog()[0].reasoning?.source).toBe("user");
    });
    expect(
      screen.getByLabelText("deepseek-v4-pro推理能力 JSON"),
    ).not.toHaveAttribute("readonly");
    expect(
      screen.getByText("能力来源：用户声明（已覆盖维护值）"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "恢复内置默认" }));
    await waitFor(() => {
      expect(latestCatalog()[0].reasoning).toEqual(builtinReasoning);
    });
    expect(
      screen.getByLabelText("deepseek-v4-pro推理能力 JSON"),
    ).toHaveAttribute("readonly");
  });
  it("syncs relay models without inferring protocols or suggesting provider splits", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "gpt-5.5", ownedBy: null, contextWindow: 272000 },
      { id: "qwen3.6", ownedBy: null, contextWindow: 128000 },
    ]);
    const {
      latestCatalog,
      latestRouting,
      onRoutingChange,
      onTakeoverEnabledChange,
      onApiFormatChange,
    } = renderAutoSplitHarness();

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));

    await waitFor(() => {
      expect(latestCatalog()).toHaveLength(2);
    });
    expect(screen.queryByText("检测到混合协议模型")).not.toBeInTheDocument();
    expect(latestCatalog()).toEqual([
      expect.objectContaining({ model: "gpt-5.5" }),
      expect.objectContaining({ model: "qwen3.6" }),
    ]);
    expect(latestCatalog()[0]).not.toHaveProperty("apiFormat");
    expect(latestCatalog()[1]).not.toHaveProperty("apiFormat");
    expect(onRoutingChange).not.toHaveBeenCalled();
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
    const records = [createDeepProbeRecord("gpt-5.5", "open_ai_responses")];
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockImplementationOnce(async (_provider, onProgress) => {
      emitFinishedProgress(records, onProgress);
      return createDeepProbeOutcome(records);
    });
    const { onApiFormatChange } = renderCatalogHarness(
      [{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }],
      { shouldShowSpeedTest: true },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    expect(screen.getByText("确认测试 Chat / Responses")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(
      await screen.findByRole("heading", { name: "Codex 兼容性深度探测" }),
    ).toBeInTheDocument();

    await waitFor(() => {
      expect(onApiFormatChange).toHaveBeenCalledWith("openai_responses");
    });
    expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "codex-thirdparty",
        settingsConfig: expect.objectContaining({
          auth: { OPENAI_API_KEY: "sk-test" },
          modelCatalog: {
            models: [expect.objectContaining({ model: "gpt-5.5" })],
          },
        }),
      }),
      expect.any(Function),
    );
  });

  it("sends the exact provider request policy to protocol preflight", async () => {
    const records = [createDeepProbeRecord("qwen3.8", "open_ai_chat")];
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockImplementationOnce(async (_provider, onProgress) => {
      emitFinishedProgress(records, onProgress);
      return createDeepProbeOutcome(records);
    });
    renderCatalogHarness(
      [{ model: "qwen3.8", upstreamModel: "Qwen/Qwen3.8" }],
      {
        providerName: "Qwen relay",
        baseUrl: "https://relay.example/v1/chat/completions",
        shouldShowSpeedTest: true,
        isFullUrl: true,
        customUserAgent: "ccswitch-probe/1.0",
        headersOverride: '{"X-Route":"qwen"}',
        bodyOverride: '{"temperature":0.2}',
        codexChatReasoning: {
          supportsThinking: true,
          supportsEffort: true,
          thinkingParam: "enable_thinking",
          effortParam: "reasoning_effort",
          effortValueMode: "passthrough",
          outputFormat: "reasoning_content",
        },
        promptCacheRouting: "enabled",
        protocolMode: "manual",
        reasoningProjection: "raw_reasoning_text",
        toolSchemaDialect: "moonshot_mfjs",
        historyReplay: "chat_reasoning_content",
      },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() => {
      expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalled();
    });
    const provider = vi.mocked(preflightCodexProviderProtocolCompatibility).mock
      .calls[0][0];
    expect(provider.meta).toEqual(
      expect.objectContaining({
        apiFormat: "openai_chat",
        isFullUrl: true,
        customUserAgent: "ccswitch-probe/1.0",
        localProxyRequestOverrides: {
          headers: { "x-route": "qwen" },
          body: { temperature: 0.2 },
        },
        codexChatReasoning: expect.objectContaining({
          supportsThinking: true,
          supportsEffort: true,
          thinkingParam: "enable_thinking",
          effortParam: "reasoning_effort",
          outputFormat: "reasoning_content",
        }),
        promptCacheRouting: "enabled",
        codexProtocolMode: "manual",
        codexReasoningProjection: "raw_reasoning_text",
        codexToolSchemaDialect: "moonshot_mfjs",
        codexHistoryReplay: "chat_reasoning_content",
      }),
    );
    expect(provider.settingsConfig.config).toContain(
      'base_url = "https://relay.example/v1/chat/completions"',
    );
    expect(provider.settingsConfig.config).toContain('wire_api = "chat"');
  });

  it("omits disabled catalog models from the deep-probe request", async () => {
    const records = [
      createDeepProbeRecord("model-enabled", "open_ai_responses"),
    ];
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockImplementationOnce(async (_provider, onProgress) => {
      emitFinishedProgress(records, onProgress);
      return createDeepProbeOutcome(records);
    });
    renderCatalogHarness(
      [
        {
          model: "model-enabled",
          upstreamModel: "model-enabled",
          enabled: true,
        },
        {
          model: "model-disabled",
          upstreamModel: "model-disabled",
          enabled: false,
        },
      ],
      { shouldShowSpeedTest: true },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() => {
      expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalled();
    });
    const provider = vi.mocked(preflightCodexProviderProtocolCompatibility).mock
      .calls[0][0];
    expect(provider.settingsConfig.modelCatalog.models).toEqual([
      expect.objectContaining({ model: "model-enabled" }),
    ]);
  });

  it("invalidates successful readiness for every routing and catalog identity input", async () => {
    vi.mocked(preflightCodexProviderProtocolCompatibility).mockImplementation(
      async (provider, onProgress) => {
        const models = provider.settingsConfig.modelCatalog.models.map(
          (model: { model: string }) => model.model,
        );
        const selectedTransport =
          provider.meta?.apiFormat === "openai_chat"
            ? "open_ai_chat"
            : "open_ai_responses";
        const records = models.map((model: string) =>
          createDeepProbeRecord(model, selectedTransport),
        );
        emitFinishedProgress(records, onProgress);
        return createDeepProbeOutcome(records);
      },
    );
    const { updateIdentity } = renderReadinessIdentityHarness();

    const validateCurrentIdentity = async () => {
      fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
      fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
      expect(await screen.findByText("可加入 MultiRouter")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "关闭" }));
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
    const oldProbe = deferred<CodexProviderProtocolPreflightOutcome>();
    const newProbe = deferred<CodexProviderProtocolPreflightOutcome>();
    vi.mocked(preflightCodexProviderProtocolCompatibility).mockImplementation(
      async (provider) =>
        String(provider.settingsConfig.config).includes("old.example")
          ? oldProbe.promise
          : newProbe.promise,
    );
    const { updateIdentity } = renderReadinessIdentityHarness();

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));
    await waitFor(() => {
      expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalledTimes(
        1,
      );
    });

    updateIdentity({ baseUrl: "https://new.example/v1" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "验证连接" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await act(async () => {
      newProbe.resolve(
        createDeepProbeOutcome([
          createDeepProbeRecord("model-a", "open_ai_responses"),
        ]),
      );
      await Promise.resolve();
    });
    expect(await screen.findByText("可加入 MultiRouter")).toBeInTheDocument();

    await act(async () => {
      oldProbe.resolve(
        createDeepProbeOutcome([
          createDeepProbeRecord("model-a", null, "unverified"),
        ]),
      );
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText("可加入 MultiRouter")).toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });
  });

  it("keeps per-model selections out of the catalog and exposes receipts to the save coordinator", async () => {
    const records = [
      createDeepProbeRecord("gpt-5.5", "open_ai_responses"),
      createDeepProbeRecord("qwen3.6", "open_ai_chat"),
      createDeepProbeRecord("glm-4.5", null, "unverified"),
    ];
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockImplementationOnce(async (_provider, onProgress) => {
      emitFinishedProgress(records, onProgress);
      return createDeepProbeOutcome(records);
    });
    const {
      latestCatalog,
      onApiFormatChange,
      onProtocolProbeReceiptIdsChange,
    } = renderCatalogHarness(
      [
        { model: "gpt-5.5", upstreamModel: "gpt-5.5" },
        { model: "qwen3.6", upstreamModel: "qwen3.6" },
        { model: "glm-4.5", upstreamModel: "glm-4.5" },
      ],
      {
        providerName: "Relay",
        shouldShowSpeedTest: true,
      },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() =>
      expect(onProtocolProbeReceiptIdsChange).toHaveBeenLastCalledWith([
        "receipt-gpt-5.5",
        "receipt-qwen3.6",
        "receipt-glm-4.5",
      ]),
    );
    expect(latestCatalog()).toEqual([
      expect.objectContaining({ model: "gpt-5.5" }),
      expect.objectContaining({ model: "qwen3.6" }),
      expect.objectContaining({ model: "glm-4.5" }),
    ]);
    expect(latestCatalog()[0]).not.toHaveProperty("apiFormat");
    expect(latestCatalog()[1]).not.toHaveProperty("apiFormat");
    expect(latestCatalog()[2]).not.toHaveProperty("apiFormat");
    expect(screen.queryByText("检测到混合协议模型")).not.toBeInTheDocument();
    expect(screen.getByText(/存在 Partial\/Failed 模型.*不能自动保存/)).toBeInTheDocument();
    expect(onApiFormatChange).not.toHaveBeenCalled();
    expect(
      screen.getByRole("article", { name: "gpt-5.5 探测进度" }),
    ).toHaveTextContent("选择 Responses");
    expect(
      screen.getByRole("article", { name: "qwen3.6 探测进度" }),
    ).toHaveTextContent("选择 Chat Completions");
    expect(
      screen.getByRole("article", { name: "glm-4.5 探测进度" }),
    ).toHaveTextContent("Failed");
  });

  it("marks a fully verified mixed-protocol provider as ready", async () => {
    const records = [
      createDeepProbeRecord("gpt-5.5", "open_ai_responses"),
      createDeepProbeRecord("qwen3.6", "open_ai_chat"),
    ];
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockImplementationOnce(async (_provider, onProgress) => {
      emitFinishedProgress(records, onProgress);
      return createDeepProbeOutcome(records);
    });
    const { onApiFormatChange, onProtocolProbeReceiptIdsChange } =
      renderCatalogHarness(
      [
        { model: "gpt-5.5", upstreamModel: "gpt-5.5" },
        { model: "qwen3.6", upstreamModel: "qwen3.6" },
      ],
      { providerName: "Relay", shouldShowSpeedTest: true },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByText("可加入 MultiRouter")).toBeInTheDocument();
    expect(screen.getByText(/保存时将自动拆分/)).toBeInTheDocument();
    expect(onApiFormatChange).not.toHaveBeenCalled();
    expect(onProtocolProbeReceiptIdsChange).toHaveBeenLastCalledWith([
      "receipt-gpt-5.5",
      "receipt-qwen3.6",
    ]);
  });

  it("keeps a model protocol unchanged when probing is only partial", async () => {
    const records = [
      createDeepProbeRecord("qwen3.8", "open_ai_chat", "partial"),
    ];
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockImplementationOnce(async (_provider, onProgress) => {
      emitFinishedProgress(records, onProgress);
      return createDeepProbeOutcome(records);
    });
    const { latestCatalog, onApiFormatChange } = renderCatalogHarness(
      [
        {
          model: "qwen3.8",
          upstreamModel: "Qwen/Qwen3.8",
          apiFormat: "openai_responses",
        },
      ],
      { shouldShowSpeedTest: true },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    await waitFor(() => {
      expect(
        screen.getByRole("article", { name: "qwen3.8 探测进度" }),
      ).toHaveTextContent("Partial");
    });
    expect(latestCatalog()).toEqual([
      expect.objectContaining({
        model: "qwen3.8",
        apiFormat: "openai_responses",
      }),
    ]);
    expect(onApiFormatChange).not.toHaveBeenCalled();
  });

  it("opens the protocol probe confirmation above the full screen provider panel", async () => {
    renderCatalogHarness([{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }], {
      shouldShowSpeedTest: true,
      openAdvancedOptions: false,
    });
    await waitForReasoningResolution();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "高级选项" }));
      fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    });

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

    expect(await screen.findByRole("status")).toHaveTextContent(
      "请先在“模型与兼容性”同步模型，或在高级设置中启用/添加至少一个模型后再验证。",
    );
    expect(screen.queryByText("确认测试 Chat / Responses")).not.toBeInTheDocument();
    const fetchButton = screen.getByRole("button", {
      name: "同步模型",
    });
    expect(fetchButton).toHaveClass("border-blue-500");
    await waitFor(() =>
      expect(HTMLElement.prototype.scrollIntoView).toHaveBeenCalled(),
    );
    expect(preflightCodexProviderProtocolCompatibility).not.toHaveBeenCalled();
  });

  it("points users to enable a model when every catalog model is disabled", async () => {
    renderCatalogHarness(
      [{ model: "disabled-model", upstreamModel: "disabled-model", enabled: false }],
      { shouldShowSpeedTest: true },
    );

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "启用/添加至少一个模型后再验证",
    );
    expect(screen.queryByText("确认测试 Chat / Responses")).not.toBeInTheDocument();
    expect(preflightCodexProviderProtocolCompatibility).not.toHaveBeenCalled();
  });

  it("surfaces protocol probe exceptions inline instead of looking frozen", async () => {
    vi.mocked(
      preflightCodexProviderProtocolCompatibility,
    ).mockRejectedValueOnce(new Error("backend timeout"));
    renderCatalogHarness([{ model: "gpt-5.5", upstreamModel: "gpt-5.5" }], {
      shouldShowSpeedTest: true,
    });

    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    fireEvent.click(screen.getByRole("button", { name: "确认测试" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "探测中断：backend timeout",
    );
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
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

  it("keeps AgentPlan catalog when both inference key and AK/SK are missing", async () => {
    const { latestCatalog } = renderCatalogHarness(
      [{ model: "ark-code-latest", upstreamModel: "ark-code-latest" }],
      {
        providerName: "火山Agentplan",
        partnerPromotionKey: "volcengine_agentplan",
        baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
        apiKey: "",
      },
    );
    await waitForReasoningResolution();

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
