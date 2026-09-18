/**
 * TE Provider 的静态设置校验与投影。
 *
 * 这里只处理**非秘密**字段，并且与注入器（ccsm-te-provider-sdk 的 TeProviderConfig）保持同一套
 * 规则：地址必须是数值回环、协议版本固定、模型能力字段要么合法要么不写（缺失=未知）。
 * Proxy Key、Agent Credential、task/lease/session/binding 都不属于这里。
 */
import type {
  OpenClawModel,
  OpenClawProviderConfig,
  OpenClawTeProviderModel,
  OpenClawTeProviderSettings,
} from "@/types";

/** 与注入器 credential_contract 中的占位值保持一致：它不是凭据。 */
export const TE_PROVIDER_PLACEHOLDER_API_KEY =
  "te-provider-placeholder-not-a-secret";

export const TE_PROVIDER_PROTOCOL_VERSION = "te-provider.v1" as const;
export const TE_PROVIDER_DEFAULT_TIMEOUT_SECONDS = 300;
export const TE_PROVIDER_DEFAULT_KEEP_ALIVE_SECONDS = 30;

const INPUT_MODALITIES = ["text", "image", "audio", "video", "file"];
const OUTPUT_MODALITIES = ["text", "embedding", "audio", "image"];
const REASONING_EFFORTS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
];

function isLoopbackHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    if (parsed.protocol !== "http:") return false;
    // 只信数值回环：localhost 依赖名称解析，可能被 hosts/DNS 重写到非本机端点。
    if (parsed.hostname !== "127.0.0.1" && parsed.hostname !== "[::1]") {
      return false;
    }
    return (
      parsed.username === "" &&
      parsed.password === "" &&
      parsed.search === "" &&
      parsed.hash === ""
    );
  } catch {
    return false;
  }
}

/**
 * 校验 TE Provider 静态设置；返回错误码数组（空数组表示通过）。
 *
 * 错误码而非中文文案：UI 负责翻译，测试断言稳定。
 */
export function validateTeProviderSettings(
  settings: OpenClawTeProviderSettings,
): string[] {
  const errors: string[] = [];
  if (!settings || typeof settings !== "object") {
    return ["te_provider_settings_required"];
  }
  if (!settings.sidecarUrl?.trim()) {
    errors.push("te_provider_sidecar_url_required");
  } else if (!isLoopbackHttpUrl(settings.sidecarUrl.trim())) {
    errors.push("te_provider_sidecar_url_must_be_loopback");
  }
  if (!settings.expectedPartnerAic?.trim()) {
    errors.push("te_provider_expected_aic_required");
  }
  if (settings.protocolVersion !== TE_PROVIDER_PROTOCOL_VERSION) {
    errors.push("te_provider_protocol_version_invalid");
  }
  if (
    settings.bindingDelivery !== "config-headers" &&
    settings.bindingDelivery !== "gateway-plugin"
  ) {
    errors.push("te_provider_binding_delivery_invalid");
  }
  const probe = settings.providerProbeUrl?.trim();
  if (probe && !isLoopbackHttpUrl(probe)) {
    errors.push("te_provider_probe_url_must_be_loopback");
  }
  if (
    settings.providerTimeoutSeconds !== undefined &&
    (!Number.isInteger(settings.providerTimeoutSeconds) ||
      settings.providerTimeoutSeconds < 30)
  ) {
    errors.push("te_provider_timeout_invalid");
  }
  if (
    settings.keepAliveIntervalSeconds !== undefined &&
    (!Number.isInteger(settings.keepAliveIntervalSeconds) ||
      settings.keepAliveIntervalSeconds < 5)
  ) {
    errors.push("te_provider_keep_alive_invalid");
  }
  if (!Array.isArray(settings.models) || settings.models.length === 0) {
    errors.push("te_provider_models_required");
  } else {
    const seen = new Set<string>();
    settings.models.forEach((model, index) => {
      const prefix = `te_provider_model_${index}`;
      if (!model?.id?.trim()) {
        errors.push(`${prefix}_id_required`);
      } else if (seen.has(model.id.trim())) {
        errors.push(`${prefix}_id_duplicated`);
      } else {
        seen.add(model.id.trim());
      }
      if (!model?.name?.trim()) errors.push(`${prefix}_name_required`);
      errors.push(
        ...validateModalityList(model?.inputModalities, INPUT_MODALITIES, `${prefix}_input`),
        ...validateModalityList(model?.outputModalities, OUTPUT_MODALITIES, `${prefix}_output`),
        ...validateModalityList(
          model?.reasoningEfforts,
          REASONING_EFFORTS,
          `${prefix}_reasoning`,
        ),
      );
      for (const field of ["contextWindowTokens", "maxOutputTokens"] as const) {
        const value = model?.[field];
        if (value === undefined) continue;
        if (!Number.isInteger(value) || value <= 0) {
          errors.push(`${prefix}_${field}_invalid`);
        }
      }
    });
  }
  return errors;
}

function validateModalityList(
  values: string[] | undefined,
  allowed: string[],
  prefix: string,
): string[] {
  if (values === undefined) return [];
  if (!Array.isArray(values)) return [`${prefix}_invalid`];
  return values.some((value) => !allowed.includes(value))
    ? [`${prefix}_invalid`]
    : [];
}

/** 把 TE 模型元数据投影成 OpenClaw 模型条目；未知字段一律省略。 */
export function toOpenClawModel(model: OpenClawTeProviderModel): OpenClawModel {
  const entry: OpenClawModel = { id: model.id, name: model.name };
  if (model.inputModalities?.length) entry.input = [...model.inputModalities];
  if (model.contextWindowTokens !== undefined) {
    entry.contextWindow = model.contextWindowTokens;
  }
  if (model.maxOutputTokens !== undefined) entry.maxTokens = model.maxOutputTokens;
  if (model.supportsReasoning !== undefined) {
    entry.reasoning = model.supportsReasoning;
  }
  if (model.cost) entry.cost = { ...model.cost };
  if (model.supportsTools !== undefined) {
    entry.compat = { ...(entry.compat ?? {}), supportsTools: model.supportsTools };
  }
  return entry;
}

/**
 * 构造写入 Agent 的 Provider 配置：baseUrl 指向本机注入端点，apiKey 固定为公开占位值。
 *
 * 调用前必须先用 validateTeProviderSettings 校验；非法设置直接抛错，不允许写入半成品配置。
 */
export function buildOpenClawTeProviderConfig(
  settings: OpenClawTeProviderSettings,
): OpenClawProviderConfig {
  const errors = validateTeProviderSettings(settings);
  if (errors.length > 0) {
    throw new Error(`invalid TE provider settings: ${errors.join(",")}`);
  }
  return {
    api: "openai-completions",
    baseUrl: `${settings.sidecarUrl.trim().replace(/\/+$/, "")}/v1`,
    apiKey: TE_PROVIDER_PLACEHOLDER_API_KEY,
    models: settings.models.map(toOpenClawModel),
    teProvider: {
      ...settings,
      sidecarUrl: settings.sidecarUrl.trim(),
      expectedPartnerAic: settings.expectedPartnerAic.trim(),
      providerTimeoutSeconds:
        settings.providerTimeoutSeconds ?? TE_PROVIDER_DEFAULT_TIMEOUT_SECONDS,
      keepAliveIntervalSeconds:
        settings.keepAliveIntervalSeconds ?? TE_PROVIDER_DEFAULT_KEEP_ALIVE_SECONDS,
    },
  };
}

export interface TeProviderBindingField {
  field: string;
  filledBy: "user" | "sdk" | "host" | "injector";
  persisted: boolean;
  note: string;
}

/**
 * 绑定设置字段清单：UI 用它区分「用户填的静态字段」与「SDK/宿主填的运行时字段」。
 * 运行时字段只读展示，且明确标注不落盘。
 */
export const TE_PROVIDER_BINDING_FIELDS: readonly TeProviderBindingField[] = [
  { field: "sidecarUrl", filledBy: "user", persisted: true, note: "本机注入端点（数值回环）" },
  { field: "expectedPartnerAic", filledBy: "user", persisted: true, note: "本 host 的 Partner AIC" },
  { field: "models", filledBy: "user", persisted: true, note: "已验证的模型与能力元数据" },
  { field: "bindingDelivery", filledBy: "user", persisted: true, note: "配置头或 Gateway 插件" },
  { field: "taskId", filledBy: "sdk", persisted: false, note: "来自 Task Package" },
  { field: "leaseId", filledBy: "injector", persisted: false, note: "每代授权一个" },
  { field: "agentId", filledBy: "host", persisted: false, note: "取自真实运行时上下文" },
  { field: "sessionKey", filledBy: "host", persisted: false, note: "取自真实运行时上下文" },
  { field: "sessionId", filledBy: "host", persisted: false, note: "取自真实运行时上下文" },
  { field: "bindingId", filledBy: "injector", persisted: false, note: "短时 capability" },
  { field: "expiresAt", filledBy: "sdk", persisted: false, note: "lease 绝对到期" },
] as const;
