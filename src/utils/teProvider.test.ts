import { describe, expect, it } from "vitest";

import type { OpenClawTeProviderSettings } from "@/types";
import {
  TE_PROVIDER_PLACEHOLDER_API_KEY,
  TE_PROVIDER_BINDING_FIELDS,
  buildOpenClawTeProviderConfig,
  canonicalizeTeProviderSettings,
  TE_PROVIDER_MAX_KEEP_ALIVE_SECONDS,
  TE_PROVIDER_MAX_TIMEOUT_SECONDS,
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

  it("rejects timeout and keep-alive values above the Rust persistence limits", () => {
    expect(
      validateTeProviderSettings(
        settings({
          providerTimeoutSeconds: TE_PROVIDER_MAX_TIMEOUT_SECONDS + 1,
          keepAliveIntervalSeconds: TE_PROVIDER_MAX_KEEP_ALIVE_SECONDS + 1,
        }),
      ),
    ).toEqual([
      "te_provider_timeout_invalid",
      "te_provider_keep_alive_invalid",
    ]);
  });

  it("drops the retired provider probe URL during canonical migration", () => {
    const migrated = canonicalizeTeProviderSettings({
      ...settings(),
      providerProbeUrl: "https://attacker.example/provider-health",
    } as unknown);
    expect(migrated).not.toBeNull();
    expect(migrated).not.toHaveProperty("providerProbeUrl");
  });

  it("rejects non-numeric loopback, credentials in URL and missing AIC", () => {
    expect(
      validateTeProviderSettings(
        settings({ sidecarUrl: "http://localhost:9814" }),
      ),
    ).toContain("te_provider_sidecar_url_must_be_loopback");
    expect(
      validateTeProviderSettings(
        settings({ sidecarUrl: "http://user:pass@127.0.0.1:9814" }),
      ),
    ).toContain("te_provider_sidecar_url_must_be_loopback");
    expect(
      validateTeProviderSettings(
        settings({ sidecarUrl: "https://127.0.0.1:9814" }),
      ),
    ).toContain("te_provider_sidecar_url_must_be_loopback");
    expect(
      validateTeProviderSettings(settings({ expectedPartnerAic: "  " })),
    ).toContain("te_provider_expected_aic_required");
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

  it("returns stable validation errors for malformed runtime JSON instead of throwing", () => {
    const malformed = {
      sidecarUrl: 42,
      expectedPartnerAic: {},
      protocolVersion: "te-provider.v1",
      bindingDelivery: "config-headers",
      models: [{ id: [], name: null }],
    } as never;
    expect(() => validateTeProviderSettings(malformed)).not.toThrow();
    expect(validateTeProviderSettings(malformed)).toEqual(
      expect.arrayContaining([
        "te_provider_sidecar_url_required",
        "te_provider_expected_aic_required",
        "te_provider_model_0_id_required",
        "te_provider_model_0_name_required",
      ]),
    );
  });

  it("rejects runtime, secret, and unknown fields instead of trusting object spreads", () => {
    const malicious = settings() as unknown as Record<string, unknown>;
    malicious.proxyKey = "secret-proxy-key";
    malicious.taskId = "runtime-task";
    malicious.unknownRuntimeField = "should-not-persist";
    expect(validateTeProviderSettings(malicious as never)).toEqual(
      expect.arrayContaining([
        "te_provider_secret_field_forbidden",
        "te_provider_runtime_field_forbidden",
        "te_provider_unknown_field_forbidden",
      ]),
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

  it("emits only the canonical static descriptor and never copies runtime fields", () => {
    const malicious = {
      ...settings(),
      taskId: "runtime-task",
      leaseId: "runtime-lease",
      proxyKey: "secret-proxy-key",
      agentCredential: "secret-agent-credential",
      unknownRuntimeField: "unknown",
    } as never;
    expect(() => buildOpenClawTeProviderConfig(malicious)).toThrow(
      /invalid TE provider settings/,
    );
    expect(
      Object.keys(
        buildOpenClawTeProviderConfig(settings()).teProvider ?? {},
      ).sort(),
    ).toEqual([
      "bindingDelivery",
      "expectedPartnerAic",
      "keepAliveIntervalSeconds",
      "models",
      "protocolVersion",
      "providerTimeoutSeconds",
      "sidecarUrl",
    ]);
  });

  it("refuses to build a config from invalid settings", () => {
    expect(() =>
      buildOpenClawTeProviderConfig(
        settings({ sidecarUrl: "http://example.com" }),
      ),
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
    const runtimeFields = TE_PROVIDER_BINDING_FIELDS.filter(
      (field) => !field.persisted,
    );
    expect(runtimeFields.map((field) => field.field)).toEqual([
      "taskId",
      "leaseId",
      "agentId",
      "sessionKey",
      "sessionId",
      "bindingId",
      "proxyKey",
      "agentCredential",
      "expiresAt",
    ]);
    // 运行时字段绝不能被标成用户填写或可持久化。
    expect(runtimeFields.every((field) => field.filledBy !== "user")).toBe(
      true,
    );
    expect(
      TE_PROVIDER_BINDING_FIELDS.filter((field) => field.persisted).map(
        (f) => f.field,
      ),
    ).toEqual([
      "sidecarUrl",
      "expectedPartnerAic",
      "protocolVersion",
      "providerTimeoutSeconds",
      "keepAliveIntervalSeconds",
      "models",
      "bindingDelivery",
    ]);
  });
});
