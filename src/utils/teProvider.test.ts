import { describe, expect, it } from "vitest";

import type { OpenClawTeProviderSettings } from "@/types";
import {
  TE_PROVIDER_PLACEHOLDER_API_KEY,
  TE_PROVIDER_BINDING_FIELDS,
  buildOpenClawTeProviderConfig,
  toOpenClawModel,
  validateTeProviderSettings,
} from "@/utils/teProvider";

function settings(
  overrides: Partial<OpenClawTeProviderSettings> = {},
): OpenClawTeProviderSettings {
  return {
    sidecarUrl: "http://127.0.0.1:9814",
    expectedPartnerAic: "1.2.156.3088.1.0001.00001.MNR0MH.TFWI94.00W7",
    protocolVersion: "te-provider.v1",
    bindingDelivery: "config-headers",
    models: [{ id: "qwen3.8", name: "Qwen 3.8" }],
    ...overrides,
  };
}

describe("validateTeProviderSettings", () => {
  it("accepts a minimal loopback descriptor", () => {
    expect(validateTeProviderSettings(settings())).toEqual([]);
  });

  it("rejects non-numeric loopback, credentials in URL and missing AIC", () => {
    expect(
      validateTeProviderSettings(settings({ sidecarUrl: "http://localhost:9814" })),
    ).toContain("te_provider_sidecar_url_must_be_loopback");
    expect(
      validateTeProviderSettings(
        settings({ sidecarUrl: "http://user:pass@127.0.0.1:9814" }),
      ),
    ).toContain("te_provider_sidecar_url_must_be_loopback");
    expect(
      validateTeProviderSettings(settings({ sidecarUrl: "https://127.0.0.1:9814" })),
    ).toContain("te_provider_sidecar_url_must_be_loopback");
    expect(validateTeProviderSettings(settings({ expectedPartnerAic: "  " }))).toContain(
      "te_provider_expected_aic_required",
    );
  });

  it("rejects unsupported modality, reasoning effort and invalid sizes", () => {
    const errors = validateTeProviderSettings(
      settings({
        models: [
          {
            id: "demo",
            name: "Demo",
            inputModalities: ["text", "hologram"],
            reasoningEfforts: ["turbo"],
            contextWindowTokens: 0,
            maxOutputTokens: -1,
          },
        ],
      }),
    );
    expect(errors).toContain("te_provider_model_0_input_invalid");
    expect(errors).toContain("te_provider_model_0_reasoning_invalid");
    expect(errors).toContain("te_provider_model_0_contextWindowTokens_invalid");
    expect(errors).toContain("te_provider_model_0_maxOutputTokens_invalid");
  });

  it("rejects duplicated model ids and empty model list", () => {
    expect(
      validateTeProviderSettings(
        settings({
          models: [
            { id: "demo", name: "A" },
            { id: "demo", name: "B" },
          ],
        }),
      ),
    ).toContain("te_provider_model_1_id_duplicated");
    expect(validateTeProviderSettings(settings({ models: [] }))).toContain(
      "te_provider_models_required",
    );
  });
});

describe("buildOpenClawTeProviderConfig", () => {
  it("points the agent at the loopback injector with a public placeholder key", () => {
    const config = buildOpenClawTeProviderConfig(settings());
    expect(config.api).toBe("openai-completions");
    expect(config.baseUrl).toBe("http://127.0.0.1:9814/v1");
    expect(config.apiKey).toBe(TE_PROVIDER_PLACEHOLDER_API_KEY);
    expect(config.models).toEqual([{ id: "qwen3.8", name: "Qwen 3.8" }]);
    // 任何真实凭据都不允许出现在这里。
    expect(JSON.stringify(config)).not.toContain("pxk");
  });

  it("applies timeout defaults and strips trailing slashes", () => {
    const config = buildOpenClawTeProviderConfig(
      settings({ sidecarUrl: "http://127.0.0.1:9814/" }),
    );
    expect(config.baseUrl).toBe("http://127.0.0.1:9814/v1");
    expect(config.teProvider?.providerTimeoutSeconds).toBe(300);
    expect(config.teProvider?.keepAliveIntervalSeconds).toBe(30);
  });

  it("refuses to build a config from invalid settings", () => {
    expect(() =>
      buildOpenClawTeProviderConfig(settings({ sidecarUrl: "http://example.com" })),
    ).toThrow(/invalid TE provider settings/);
  });
});

describe("toOpenClawModel", () => {
  it("projects verified capability metadata using agent-side field names", () => {
    expect(
      toOpenClawModel({
        id: "qwen3.8",
        name: "Qwen 3.8",
        inputModalities: ["text", "image"],
        contextWindowTokens: 262144,
        maxOutputTokens: 32768,
        supportsReasoning: true,
        supportsTools: true,
        cost: { input: 0, output: 0 },
      }),
    ).toEqual({
      id: "qwen3.8",
      name: "Qwen 3.8",
      input: ["text", "image"],
      contextWindow: 262144,
      maxTokens: 32768,
      reasoning: true,
      cost: { input: 0, output: 0 },
      compat: { supportsTools: true },
    });
  });

  it("omits unknown capabilities instead of guessing", () => {
    expect(toOpenClawModel({ id: "mystery", name: "mystery" })).toEqual({
      id: "mystery",
      name: "mystery",
    });
  });
});

describe("binding fields", () => {
  it("separates user-filled static fields from runtime-only fields", () => {
    const runtimeFields = TE_PROVIDER_BINDING_FIELDS.filter((field) => !field.persisted);
    expect(runtimeFields.map((field) => field.field)).toEqual([
      "taskId",
      "leaseId",
      "agentId",
      "sessionKey",
      "sessionId",
      "bindingId",
      "expiresAt",
    ]);
    // 运行时字段绝不能被标成用户填写或可持久化。
    expect(runtimeFields.every((field) => field.filledBy !== "user")).toBe(true);
    expect(
      TE_PROVIDER_BINDING_FIELDS.filter((field) => field.persisted).map((f) => f.field),
    ).toEqual(["sidecarUrl", "expectedPartnerAic", "models", "bindingDelivery"]);
  });
});
