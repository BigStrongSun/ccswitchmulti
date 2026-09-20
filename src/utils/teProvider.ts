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
export const TE_PROVIDER_MAX_TIMEOUT_SECONDS = 86_400;
export const TE_PROVIDER_MAX_KEEP_ALIVE_SECONDS = 3_600;

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

const TE_STATIC_FIELDS = new Set([
  "sidecarUrl",
  "expectedPartnerAic",
  "protocolVersion",
  "bindingDelivery",
  "providerTimeoutSeconds",
  "keepAliveIntervalSeconds",
  "models",
]);
// 旧版本曾保存过 providerProbeUrl；迁移时允许读取，但 canonicalize 会丢弃。
const TE_LEGACY_FIELDS = new Set(["providerProbeUrl"]);
const TE_RUNTIME_FIELDS = new Set([
  "taskId",
  "leaseId",
  "sessionKey",
  "sessionId",
  "bindingId",
  "agentId",
  "expiresAt",
]);
const TE_SECRET_FIELDS = new Set(["proxyKey", "agentCredential"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

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
export function validateTeProviderSettings(settings: unknown): string[] {
  const errors: string[] = [];
  if (!isRecord(settings)) return ["te_provider_settings_required"];

  for (const field of Object.keys(settings)) {
    if (TE_RUNTIME_FIELDS.has(field))
      errors.push("te_provider_runtime_field_forbidden");
    else if (TE_SECRET_FIELDS.has(field))
      errors.push("te_provider_secret_field_forbidden");
    else if (!TE_STATIC_FIELDS.has(field) && !TE_LEGACY_FIELDS.has(field))
      errors.push("te_provider_unknown_field_forbidden");
  }

  const sidecarUrl = stringValue(settings.sidecarUrl);
  if (!sidecarUrl?.trim()) errors.push("te_provider_sidecar_url_required");
  else if (!isLoopbackHttpUrl(sidecarUrl.trim()))
    errors.push("te_provider_sidecar_url_must_be_loopback");

  const expectedPartnerAic = stringValue(settings.expectedPartnerAic);
  if (!expectedPartnerAic?.trim())
    errors.push("te_provider_expected_aic_required");
  else if (
    expectedPartnerAic.trim().length > 256 ||
    !/^[A-Za-z0-9._:-]+$/.test(expectedPartnerAic.trim())
  )
    errors.push("te_provider_expected_aic_invalid");
  if (settings.protocolVersion !== TE_PROVIDER_PROTOCOL_VERSION)
    errors.push("te_provider_protocol_version_invalid");
  if (
    settings.bindingDelivery !== "config-headers" &&
    settings.bindingDelivery !== "gateway-plugin"
  ) {
    errors.push("te_provider_binding_delivery_invalid");
  }

  const timeout = settings.providerTimeoutSeconds;
  if (
    timeout !== undefined &&
    (typeof timeout !== "number" ||
      !Number.isInteger(timeout) ||
      timeout < 30 ||
      timeout > TE_PROVIDER_MAX_TIMEOUT_SECONDS)
  )
    errors.push("te_provider_timeout_invalid");
  const keepAlive = settings.keepAliveIntervalSeconds;
  if (
    keepAlive !== undefined &&
    (typeof keepAlive !== "number" ||
      !Number.isInteger(keepAlive) ||
      keepAlive < 5 ||
      keepAlive > TE_PROVIDER_MAX_KEEP_ALIVE_SECONDS)
  )
    errors.push("te_provider_keep_alive_invalid");

  if (!Array.isArray(settings.models) || settings.models.length === 0) {
    errors.push("te_provider_models_required");
  } else {
    const seen = new Set<string>();
    settings.models.forEach((model, index) => {
      const prefix = `te_provider_model_${index}`;
      if (!isRecord(model)) {
        errors.push(`${prefix}_invalid`);
        return;
      }
      for (const field of Object.keys(model)) {
        if (TE_RUNTIME_FIELDS.has(field))
          errors.push("te_provider_runtime_field_forbidden");
        else if (TE_SECRET_FIELDS.has(field))
          errors.push("te_provider_secret_field_forbidden");
        else if (
          ![
            "id",
            "name",
            "inputModalities",
            "outputModalities",
            "contextWindowTokens",
            "maxOutputTokens",
            "supportsTools",
            "supportsReasoning",
            "reasoningEfforts",
            "cost",
          ].includes(field)
        )
          errors.push("te_provider_unknown_field_forbidden");
      }
      const id = stringValue(model.id);
      const name = stringValue(model.name);
      if (!id?.trim()) errors.push(`${prefix}_id_required`);
      else if (seen.has(id.trim())) errors.push(`${prefix}_id_duplicated`);
      else seen.add(id.trim());
      if (!name?.trim()) errors.push(`${prefix}_name_required`);
      errors.push(
        ...validateModalityList(
          model.inputModalities,
          INPUT_MODALITIES,
          `${prefix}_input`,
        ),
        ...validateModalityList(
          model.outputModalities,
          OUTPUT_MODALITIES,
          `${prefix}_output`,
        ),
        ...validateModalityList(
          model.reasoningEfforts,
          REASONING_EFFORTS,
          `${prefix}_reasoning`,
        ),
      );
      for (const field of ["contextWindowTokens", "maxOutputTokens"] as const) {
        const value = model[field];
        if (
          value !== undefined &&
          (typeof value !== "number" || !Number.isInteger(value) || value <= 0)
        )
          errors.push(`${prefix}_${field}_invalid`);
      }
      if (model.cost !== undefined) {
        if (!isRecord(model.cost)) errors.push(`${prefix}_cost_invalid`);
        else {
          for (const field of Object.keys(model.cost)) {
            if (!["input", "output", "cacheRead", "cacheWrite"].includes(field))
              errors.push("te_provider_unknown_field_forbidden");
          }
          for (const field of [
            "input",
            "output",
            "cacheRead",
            "cacheWrite",
          ] as const) {
            const value = model.cost[field];
            if (
              value !== undefined &&
              (typeof value !== "number" ||
                !Number.isFinite(value) ||
                value < 0)
            )
              errors.push(`${prefix}_cost_invalid`);
          }
        }
      }
    });
  }
  return Array.from(new Set(errors));
}

function validateModalityList(
  values: unknown,
  allowed: readonly string[],
  prefix: string,
): string[] {
  if (values === undefined) return [];
  if (!Array.isArray(values)) return [`${prefix}_invalid`];
  return values.some(
    (value) => typeof value !== "string" || !allowed.includes(value),
  )
    ? [`${prefix}_invalid`]
    : [];
}

/** 只保留静态字段，供表单继续编辑不完整草稿；不会把运行时字段带入配置。 */
export function sanitizeTeProviderDraft(
  value: unknown,
): OpenClawTeProviderSettings | null {
  if (!isRecord(value)) return null;
  const models = Array.isArray(value.models)
    ? value.models.filter(isRecord).map((model) => {
        const result: OpenClawTeProviderModel = {
          id: stringValue(model.id) ?? "",
          name: stringValue(model.name) ?? "",
        };
        if (Array.isArray(model.inputModalities))
          result.inputModalities = model.inputModalities.filter(
            (item): item is string => typeof item === "string",
          );
        if (Array.isArray(model.outputModalities))
          result.outputModalities = model.outputModalities.filter(
            (item): item is string => typeof item === "string",
          );
        if (Array.isArray(model.reasoningEfforts))
          result.reasoningEfforts = model.reasoningEfforts.filter(
            (item): item is string => typeof item === "string",
          );
        for (const [input, output] of [
          ["contextWindowTokens", "contextWindowTokens"],
          ["maxOutputTokens", "maxOutputTokens"],
        ] as const) {
          const number = finiteNumber(model[input]);
          if (number !== undefined) result[output] = number;
        }
        if (typeof model.supportsTools === "boolean")
          result.supportsTools = model.supportsTools;
        if (typeof model.supportsReasoning === "boolean")
          result.supportsReasoning = model.supportsReasoning;
        if (isRecord(model.cost)) {
          const cost: NonNullable<OpenClawTeProviderModel["cost"]> = {
            input: finiteNumber(model.cost.input) ?? 0,
            output: finiteNumber(model.cost.output) ?? 0,
          };
          const cacheRead = finiteNumber(model.cost.cacheRead);
          const cacheWrite = finiteNumber(model.cost.cacheWrite);
          if (cacheRead !== undefined) cost.cacheRead = cacheRead;
          if (cacheWrite !== undefined) cost.cacheWrite = cacheWrite;
          result.cost = cost;
        }
        return result;
      })
    : [];
  const draft: OpenClawTeProviderSettings = {
    sidecarUrl: stringValue(value.sidecarUrl) ?? "",
    expectedPartnerAic: stringValue(value.expectedPartnerAic) ?? "",
    protocolVersion: TE_PROVIDER_PROTOCOL_VERSION,
    bindingDelivery:
      value.bindingDelivery === "gateway-plugin"
        ? "gateway-plugin"
        : "config-headers",
    models,
  };
  const timeout = finiteNumber(value.providerTimeoutSeconds);
  if (timeout !== undefined) draft.providerTimeoutSeconds = timeout;
  const keepAlive = finiteNumber(value.keepAliveIntervalSeconds);
  if (keepAlive !== undefined) draft.keepAliveIntervalSeconds = keepAlive;
  return draft;
}

/** 返回通过严格校验的全新静态描述符；输入对象的未知字段不会被复制。 */
export function canonicalizeTeProviderSettings(
  value: unknown,
): OpenClawTeProviderSettings | null {
  if (validateTeProviderSettings(value).length > 0) return null;
  const draft = sanitizeTeProviderDraft(value);
  if (!draft) return null;
  return {
    sidecarUrl: draft.sidecarUrl.trim().replace(/\/+$/, ""),
    expectedPartnerAic: draft.expectedPartnerAic.trim(),
    protocolVersion: TE_PROVIDER_PROTOCOL_VERSION,
    bindingDelivery: draft.bindingDelivery,
    providerTimeoutSeconds:
      draft.providerTimeoutSeconds ?? TE_PROVIDER_DEFAULT_TIMEOUT_SECONDS,
    keepAliveIntervalSeconds:
      draft.keepAliveIntervalSeconds ?? TE_PROVIDER_DEFAULT_KEEP_ALIVE_SECONDS,
    models: draft.models.map((model) => ({
      id: model.id.trim(),
      name: model.name.trim(),
      ...(model.inputModalities
        ? { inputModalities: [...model.inputModalities] }
        : {}),
      ...(model.outputModalities
        ? { outputModalities: [...model.outputModalities] }
        : {}),
      ...(model.contextWindowTokens !== undefined
        ? { contextWindowTokens: model.contextWindowTokens }
        : {}),
      ...(model.maxOutputTokens !== undefined
        ? { maxOutputTokens: model.maxOutputTokens }
        : {}),
      ...(model.supportsTools !== undefined
        ? { supportsTools: model.supportsTools }
        : {}),
      ...(model.supportsReasoning !== undefined
        ? { supportsReasoning: model.supportsReasoning }
        : {}),
      ...(model.reasoningEfforts
        ? { reasoningEfforts: [...model.reasoningEfforts] }
        : {}),
      ...(model.cost
        ? {
            cost: {
              input: model.cost.input,
              output: model.cost.output,
              ...(model.cost.cacheRead !== undefined
                ? { cacheRead: model.cost.cacheRead }
                : {}),
              ...(model.cost.cacheWrite !== undefined
                ? { cacheWrite: model.cost.cacheWrite }
                : {}),
            },
          }
        : {}),
    })),
  };
}

/** 把 TE 模型元数据投影成 OpenClaw 模型条目；未知字段一律省略。 */
export function toOpenClawModel(model: OpenClawTeProviderModel): OpenClawModel {
  const entry: OpenClawModel = { id: model.id, name: model.name };
  if (model.inputModalities?.length) entry.input = [...model.inputModalities];
  if (model.contextWindowTokens !== undefined) {
    entry.contextWindow = model.contextWindowTokens;
  }
  if (model.maxOutputTokens !== undefined)
    entry.maxTokens = model.maxOutputTokens;
  if (model.supportsReasoning !== undefined) {
    entry.reasoning = model.supportsReasoning;
  }
  if (model.cost) {
    entry.cost = {
      input: model.cost.input,
      output: model.cost.output,
      ...(model.cost.cacheRead !== undefined
        ? { cacheRead: model.cost.cacheRead }
        : {}),
      ...(model.cost.cacheWrite !== undefined
        ? { cacheWrite: model.cost.cacheWrite }
        : {}),
    };
  }
  if (model.supportsTools !== undefined) {
    entry.compat = {
      ...(entry.compat ?? {}),
      supportsTools: model.supportsTools,
    };
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
  const canonical = canonicalizeTeProviderSettings(settings);
  if (!canonical) {
    const errors = validateTeProviderSettings(settings);
    throw new Error(`invalid TE provider settings: ${errors.join(",")}`);
  }
  return {
    api: "openai-completions",
    baseUrl: `${canonical.sidecarUrl}/v1`,
    apiKey: TE_PROVIDER_PLACEHOLDER_API_KEY,
    models: canonical.models.map(toOpenClawModel),
    teProvider: {
      sidecarUrl: canonical.sidecarUrl,
      expectedPartnerAic: canonical.expectedPartnerAic,
      protocolVersion: canonical.protocolVersion,
      bindingDelivery: canonical.bindingDelivery,
      providerTimeoutSeconds: canonical.providerTimeoutSeconds,
      keepAliveIntervalSeconds: canonical.keepAliveIntervalSeconds,
      models: canonical.models.map((model) => ({
        id: model.id,
        name: model.name,
        ...(model.inputModalities
          ? { inputModalities: [...model.inputModalities] }
          : {}),
        ...(model.outputModalities
          ? { outputModalities: [...model.outputModalities] }
          : {}),
        ...(model.contextWindowTokens !== undefined
          ? { contextWindowTokens: model.contextWindowTokens }
          : {}),
        ...(model.maxOutputTokens !== undefined
          ? { maxOutputTokens: model.maxOutputTokens }
          : {}),
        ...(model.supportsTools !== undefined
          ? { supportsTools: model.supportsTools }
          : {}),
        ...(model.supportsReasoning !== undefined
          ? { supportsReasoning: model.supportsReasoning }
          : {}),
        ...(model.reasoningEfforts
          ? { reasoningEfforts: [...model.reasoningEfforts] }
          : {}),
        ...(model.cost
          ? {
              cost: {
                input: model.cost.input,
                output: model.cost.output,
                ...(model.cost.cacheRead !== undefined
                  ? { cacheRead: model.cost.cacheRead }
                  : {}),
                ...(model.cost.cacheWrite !== undefined
                  ? { cacheWrite: model.cost.cacheWrite }
                  : {}),
              },
            }
          : {}),
      })),
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
  {
    field: "sidecarUrl",
    filledBy: "user",
    persisted: true,
    note: "本机注入端点（数值回环）",
  },
  {
    field: "expectedPartnerAic",
    filledBy: "user",
    persisted: true,
    note: "本 host 的 Partner AIC",
  },
  {
    field: "protocolVersion",
    filledBy: "user",
    persisted: true,
    note: "固定的 TE Provider 协议版本",
  },
  {
    field: "providerTimeoutSeconds",
    filledBy: "user",
    persisted: true,
    note: "Task 超时上限（30–86400 秒）",
  },
  {
    field: "keepAliveIntervalSeconds",
    filledBy: "user",
    persisted: true,
    note: "注入器保活间隔（5–3600 秒）",
  },
  {
    field: "models",
    filledBy: "user",
    persisted: true,
    note: "已验证的模型与能力元数据",
  },
  {
    field: "bindingDelivery",
    filledBy: "user",
    persisted: true,
    note: "配置头或 Gateway 插件",
  },
  {
    field: "taskId",
    filledBy: "sdk",
    persisted: false,
    note: "来自 Task Package",
  },
  {
    field: "leaseId",
    filledBy: "injector",
    persisted: false,
    note: "每代授权一个",
  },
  {
    field: "agentId",
    filledBy: "host",
    persisted: false,
    note: "取自真实运行时上下文",
  },
  {
    field: "sessionKey",
    filledBy: "host",
    persisted: false,
    note: "取自真实运行时上下文",
  },
  {
    field: "sessionId",
    filledBy: "host",
    persisted: false,
    note: "取自真实运行时上下文",
  },
  {
    field: "bindingId",
    filledBy: "injector",
    persisted: false,
    note: "短时 capability",
  },
  {
    field: "proxyKey",
    filledBy: "injector",
    persisted: false,
    note: "由注入器按 Task/session 注入",
  },
  {
    field: "agentCredential",
    filledBy: "sdk",
    persisted: false,
    note: "由 SDK/注入器按会话注入",
  },
  {
    field: "expiresAt",
    filledBy: "sdk",
    persisted: false,
    note: "lease 绝对到期",
  },
] as const;
