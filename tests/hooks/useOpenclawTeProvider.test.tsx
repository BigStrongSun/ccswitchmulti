import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useOpenclawFormState } from "@/components/providers/forms/hooks/useOpenclawFormState";
import type { OpenClawTeProviderSettings } from "@/types";
import { TE_PROVIDER_PLACEHOLDER_API_KEY } from "@/utils/teProvider";

const teProviderQueryMock = vi.hoisted(() => ({
  data: { providers: {} },
}));

vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => teProviderQueryMock,
}));

function renderOpenclawFormState(
  initialSettingsConfig: Record<string, unknown>,
) {
  let settingsConfig = JSON.stringify(initialSettingsConfig);
  const onSettingsConfigChange = vi.fn((next: string) => {
    settingsConfig = next;
  });

  const hook = renderHook(() =>
    useOpenclawFormState({
      appId: "openclaw",
      initialData: { settingsConfig: initialSettingsConfig },
      onSettingsConfigChange,
      getSettingsConfig: () => settingsConfig,
    }),
  );

  return {
    ...hook,
    readSettingsConfig: () => JSON.parse(settingsConfig) as Record<string, any>,
  };
}

function validSettings(): OpenClawTeProviderSettings {
  return {
    sidecarUrl: "http://127.0.0.1:19001",
    expectedPartnerAic: "1.2.156.3088.0001.00001.TTLIHU.LW9WCA.1.0N2P",
    protocolVersion: "te-provider.v1",
    bindingDelivery: "config-headers",
    models: [
      {
        id: "qwen3.8",
        name: "qwen3.8",
        inputModalities: ["text"],
        outputModalities: ["text"],
      },
    ],
  };
}

describe("useOpenclawFormState TE Provider", () => {
  it("hydrates teProvider from the stored settings config", () => {
    const settings = validSettings();
    const { result } = renderOpenclawFormState({
      baseUrl: "http://127.0.0.1:9814/v1",
      apiKey: TE_PROVIDER_PLACEHOLDER_API_KEY,
      teProvider: settings,
    });

    expect(result.current.openclawTeProvider).toEqual(settings);
  });

  it("projects valid settings into the Agent-read baseUrl/apiKey/models", () => {
    const { result, readSettingsConfig } = renderOpenclawFormState({
      baseUrl: "http://127.0.0.1:9814/v1",
      apiKey: TE_PROVIDER_PLACEHOLDER_API_KEY,
      models: [],
      teProvider: validSettings(),
    });

    act(() => {
      result.current.handleOpenclawTeProviderChange({
        ...validSettings(),
        sidecarUrl: "http://127.0.0.1:19001/",
        models: [
          {
            id: "qwen3.8",
            name: "qwen3.8",
            inputModalities: ["text", "image"],
            outputModalities: ["text"],
            contextWindowTokens: 131072,
          },
        ],
      });
    });

    const stored = readSettingsConfig();
    expect(stored.teProvider.sidecarUrl).toBe("http://127.0.0.1:19001/");
    expect(stored.baseUrl).toBe("http://127.0.0.1:19001/v1");
    expect(stored.apiKey).toBe(TE_PROVIDER_PLACEHOLDER_API_KEY);
    expect(stored.models[0]).toMatchObject({
      id: "qwen3.8",
      input: ["text", "image"],
      contextWindow: 131072,
    });
    expect(result.current.openclawBaseUrl).toBe("http://127.0.0.1:19001/v1");
  });

  it("keeps the previous projection when settings are invalid", () => {
    const { result, readSettingsConfig } = renderOpenclawFormState({
      baseUrl: "http://127.0.0.1:9814/v1",
      apiKey: TE_PROVIDER_PLACEHOLDER_API_KEY,
      models: [{ id: "approved-model-id", name: "approved-model-id" }],
      teProvider: validSettings(),
    });

    act(() => {
      result.current.handleOpenclawTeProviderChange({
        ...validSettings(),
        sidecarUrl: "https://example.com",
      });
    });

    const stored = readSettingsConfig();
    // 中间态仍然保存，避免用户输入丢失；但 Agent 读取的端点不被非法值覆盖。
    expect(stored.teProvider.sidecarUrl).toBe("https://example.com");
    expect(stored.baseUrl).toBe("http://127.0.0.1:9814/v1");
    expect(stored.models).toEqual([
      { id: "approved-model-id", name: "approved-model-id" },
    ]);
  });
});
