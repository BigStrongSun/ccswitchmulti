/**
 * TE Provider 专用面板：编辑非秘密的绑定设置与模型能力元数据。
 *
 * 设计边界：
 * - 这里只写静态字段。Proxy Key、Agent Credential、task/lease/session/binding 由运行时注入，
 *   面板只展示它们「由谁填写、是否落盘」，绝不提供输入框。
 * - 模型能力缺失表示「未知」，不等于「不支持」；面板允许留空，并在校验失败时给出确定性错误码。
 */
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  OpenClawTeBindingDelivery,
  OpenClawTeProviderModel,
  OpenClawTeProviderSettings,
} from "@/types";
import {
  TE_PROVIDER_BINDING_FIELDS,
  TE_PROVIDER_DEFAULT_KEEP_ALIVE_SECONDS,
  TE_PROVIDER_DEFAULT_TIMEOUT_SECONDS,
  validateTeProviderSettings,
} from "@/utils/teProvider";
import {
  getTeProviderRuntimeStatus,
  type TeProviderRuntimeStatus,
} from "@/lib/api/teProvider";

const INPUT_MODALITIES = ["text", "image", "audio", "video", "file"] as const;
const OUTPUT_MODALITIES = ["text", "embedding", "audio", "image"] as const;
const REASONING_EFFORTS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

/** 校验错误码 → 中文说明；未登记的错误码原样显示，便于排查而不是静默吞掉。 */
const ERROR_LABELS: Record<string, string> = {
  te_provider_settings_required: "缺少 TE Provider 设置对象",
  te_provider_sidecar_url_required: "必须填写本机注入端点",
  te_provider_sidecar_url_must_be_loopback: "注入端点必须是数值回环 http://127.0.0.1 或 http://[::1]",
  te_provider_expected_aic_required: "必须填写本 host 服务的 Partner AIC",
  te_provider_protocol_version_invalid: "协议版本必须是 te-provider.v1",
  te_provider_binding_delivery_invalid: "绑定投递方式必须是配置头或 Gateway 插件",
  te_provider_probe_url_must_be_loopback: "保活探针地址必须是数值回环",
  te_provider_timeout_invalid: "Provider 超时必须是不小于 30 的整数秒",
  te_provider_keep_alive_invalid: "保活间隔必须是不小于 5 的整数秒",
  te_provider_models_required: "至少需要一个模型条目",
};

function errorLabel(code: string): string {
  if (ERROR_LABELS[code]) return ERROR_LABELS[code];
  if (code.includes("_id_required")) return "模型 ID 不能为空";
  if (code.includes("_id_duplicated")) return "模型 ID 不能重复";
  if (code.includes("_name_required")) return "模型显示名称不能为空";
  if (code.includes("_input_invalid")) return "输入模态只能是 text/image/audio/video/file";
  if (code.includes("_output_invalid")) return "输出模态只能是 text/embedding/audio/image";
  if (code.includes("_reasoning_invalid")) return "推理档位取值非法";
  if (code.includes("_contextWindowTokens_invalid")) return "上下文长度必须是正整数";
  if (code.includes("_maxOutputTokens_invalid")) return "最大输出长度必须是正整数";
  return code;
}

export interface TeProviderFieldsProps {
  value: OpenClawTeProviderSettings;
  onChange: (next: OpenClawTeProviderSettings) => void;
}

export function TeProviderFields({ value, onChange }: TeProviderFieldsProps) {
  const { t } = useTranslation();
  const errors = useMemo(() => validateTeProviderSettings(value), [value]);
  const runtimeFields = TE_PROVIDER_BINDING_FIELDS.filter(
    (field) => !field.persisted,
  );
  const [runtimeStatus, setRuntimeStatus] = useState<TeProviderRuntimeStatus | null>(
    null,
  );
  const [runtimeError, setRuntimeError] = useState("");
  const [runtimeBusy, setRuntimeBusy] = useState(false);

  /**
   * 读取运行态只做只读探针：失败时把稳定错误显示出来，不把失败伪装成「未运行」，
   * 也不因为探针失败改动任何保存中的设置。
   */
  const refreshRuntimeStatus = useCallback(async () => {
    setRuntimeBusy(true);
    setRuntimeError("");
    try {
      const status = await getTeProviderRuntimeStatus(value.sidecarUrl);
      setRuntimeStatus(status);
    } catch (error) {
      setRuntimeStatus(null);
      setRuntimeError(error instanceof Error ? error.message : "运行态读取失败");
    } finally {
      setRuntimeBusy(false);
    }
  }, [value.sidecarUrl]);

  function providerStatusText(status: TeProviderRuntimeStatus): string {
    if (!status.provider) return "上游探针未返回结果";
    if (status.provider.online === true) return "上游在线";
    if (status.provider.online === false) return "上游离线";
    return `上游状态未知（${status.provider.reason ?? "provider_probe_not_configured"}）`;
  }

  function update(patch: Partial<OpenClawTeProviderSettings>) {
    onChange({ ...value, ...patch });
  }

  function updateModel(index: number, patch: Partial<OpenClawTeProviderModel>) {
    const models = value.models.map((model, current) =>
      current === index ? { ...model, ...patch } : model,
    );
    update({ models });
  }

  function toggleList(
    current: string[] | undefined,
    item: string,
    checked: boolean,
  ): string[] {
    const next = new Set(current ?? []);
    if (checked) next.add(item);
    else next.delete(item);
    return Array.from(next);
  }

  function numberOrUndefined(raw: string): number | undefined {
    if (!raw.trim()) return undefined;
    const parsed = Number(raw);
    return Number.isFinite(parsed) ? parsed : undefined;
  }

  return (
    <div className="space-y-6" data-testid="te-provider-fields">
      <div className="rounded-lg border p-4 space-y-4">
        <div>
          <h3 className="text-sm font-medium">
            {t("openclaw.teProvider.bindings", {
              defaultValue: "TE 绑定设置（静态、无秘密）",
            })}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("openclaw.teProvider.bindingsHint", {
              defaultValue:
                "这里只保存本机注入端点与能力元数据；Proxy Key 与 Agent Credential 永远由运行时注入，不进入配置。",
            })}
          </p>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor="te-sidecar-url">
              {t("openclaw.teProvider.sidecarUrl", { defaultValue: "注入端点" })}
            </Label>
            <Input
              id="te-sidecar-url"
              data-testid="te-sidecar-url"
              value={value.sidecarUrl}
              onChange={(event) => update({ sidecarUrl: event.target.value })}
              placeholder="http://127.0.0.1:9814"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="te-partner-aic">
              {t("openclaw.teProvider.expectedPartnerAic", {
                defaultValue: "本 host Partner AIC",
              })}
            </Label>
            <Input
              id="te-partner-aic"
              data-testid="te-expected-partner-aic"
              value={value.expectedPartnerAic}
              onChange={(event) =>
                update({ expectedPartnerAic: event.target.value })
              }
              placeholder="1.2.156.3088...."
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="te-binding-delivery">
              {t("openclaw.teProvider.bindingDelivery", {
                defaultValue: "绑定投递方式",
              })}
            </Label>
            <Select
              value={value.bindingDelivery}
              onValueChange={(next) =>
                update({ bindingDelivery: next as OpenClawTeBindingDelivery })
              }
            >
              <SelectTrigger id="te-binding-delivery" data-testid="te-binding-delivery">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="config-headers">
                  {t("openclaw.teProvider.bindingDeliveryHeaders", {
                    defaultValue: "写入 Agent 配置头",
                  })}
                </SelectItem>
                <SelectItem value="gateway-plugin">
                  {t("openclaw.teProvider.bindingDeliveryPlugin", {
                    defaultValue: "OpenClaw Gateway 插件",
                  })}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="te-probe-url">
              {t("openclaw.teProvider.providerProbeUrl", {
                defaultValue: "上游保活探针（可选）",
              })}
            </Label>
            <Input
              id="te-probe-url"
              data-testid="te-provider-probe-url"
              value={value.providerProbeUrl ?? ""}
              onChange={(event) =>
                update({
                  providerProbeUrl: event.target.value.trim()
                    ? event.target.value
                    : undefined,
                })
              }
              placeholder="http://127.0.0.1:5001/v1"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="te-provider-timeout">
              {t("openclaw.teProvider.providerTimeoutSeconds", {
                defaultValue: "Provider 超时（秒）",
              })}
            </Label>
            <Input
              id="te-provider-timeout"
              data-testid="te-provider-timeout"
              inputMode="numeric"
              value={value.providerTimeoutSeconds ?? ""}
              onChange={(event) =>
                update({
                  providerTimeoutSeconds: numberOrUndefined(event.target.value),
                })
              }
              placeholder={String(TE_PROVIDER_DEFAULT_TIMEOUT_SECONDS)}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="te-keep-alive">
              {t("openclaw.teProvider.keepAliveIntervalSeconds", {
                defaultValue: "保活间隔（秒）",
              })}
            </Label>
            <Input
              id="te-keep-alive"
              data-testid="te-keep-alive"
              inputMode="numeric"
              value={value.keepAliveIntervalSeconds ?? ""}
              onChange={(event) =>
                update({
                  keepAliveIntervalSeconds: numberOrUndefined(
                    event.target.value,
                  ),
                })
              }
              placeholder={String(TE_PROVIDER_DEFAULT_KEEP_ALIVE_SECONDS)}
            />
          </div>
        </div>
      </div>

      <div className="rounded-lg border p-4 space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-sm font-medium">
              {t("openclaw.teProvider.models", {
                defaultValue: "模型能力元数据",
              })}
            </h3>
            <p className="text-xs text-muted-foreground">
              {t("openclaw.teProvider.modelsHint", {
                defaultValue:
                  "留空表示未知，不会据此推断模型能力；只有平台已验证的能力才能作为批准依据。",
              })}
            </p>
          </div>
          <button
            type="button"
            className="text-xs underline"
            data-testid="te-add-model"
            onClick={() =>
              update({
                models: [...value.models, { id: "", name: "" }],
              })
            }
          >
            {t("openclaw.teProvider.addModel", { defaultValue: "添加模型" })}
          </button>
        </div>

        {value.models.map((model, index) => (
          <div
            key={`${index}-${model.id}`}
            className="rounded-md border p-3 space-y-3"
            data-testid="te-model-row"
          >
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-2">
                <Label>{t("openclaw.teProvider.modelId", { defaultValue: "模型 ID" })}</Label>
                <Input
                  data-testid="te-model-id"
                  value={model.id}
                  onChange={(event) =>
                    updateModel(index, { id: event.target.value })
                  }
                />
              </div>
              <div className="space-y-2">
                <Label>{t("openclaw.teProvider.modelName", { defaultValue: "显示名称" })}</Label>
                <Input
                  data-testid="te-model-name"
                  value={model.name}
                  onChange={(event) =>
                    updateModel(index, { name: event.target.value })
                  }
                />
              </div>
              <div className="space-y-2">
                <Label>
                  {t("openclaw.teProvider.contextWindowTokens", {
                    defaultValue: "上下文长度（Token）",
                  })}
                </Label>
                <Input
                  data-testid="te-model-context-window"
                  inputMode="numeric"
                  value={model.contextWindowTokens ?? ""}
                  onChange={(event) =>
                    updateModel(index, {
                      contextWindowTokens: numberOrUndefined(event.target.value),
                    })
                  }
                />
              </div>
              <div className="space-y-2">
                <Label>
                  {t("openclaw.teProvider.maxOutputTokens", {
                    defaultValue: "最大输出（Token）",
                  })}
                </Label>
                <Input
                  data-testid="te-model-max-output"
                  inputMode="numeric"
                  value={model.maxOutputTokens ?? ""}
                  onChange={(event) =>
                    updateModel(index, {
                      maxOutputTokens: numberOrUndefined(event.target.value),
                    })
                  }
                />
              </div>
            </div>

            <div className="space-y-2">
              <Label>{t("openclaw.teProvider.inputModalities", { defaultValue: "输入模态" })}</Label>
              <div className="flex flex-wrap gap-3">
                {INPUT_MODALITIES.map((item) => (
                  <label key={item} className="flex items-center gap-2 text-xs">
                    <Checkbox
                      data-testid={`te-model-input-${item}`}
                      checked={(model.inputModalities ?? []).includes(item)}
                      onCheckedChange={(checked) =>
                        updateModel(index, {
                          inputModalities: toggleList(
                            model.inputModalities,
                            item,
                            checked === true,
                          ),
                        })
                      }
                    />
                    {item}
                  </label>
                ))}
              </div>
            </div>

            <div className="space-y-2">
              <Label>{t("openclaw.teProvider.outputModalities", { defaultValue: "输出模态" })}</Label>
              <div className="flex flex-wrap gap-3">
                {OUTPUT_MODALITIES.map((item) => (
                  <label key={item} className="flex items-center gap-2 text-xs">
                    <Checkbox
                      data-testid={`te-model-output-${item}`}
                      checked={(model.outputModalities ?? []).includes(item)}
                      onCheckedChange={(checked) =>
                        updateModel(index, {
                          outputModalities: toggleList(
                            model.outputModalities,
                            item,
                            checked === true,
                          ),
                        })
                      }
                    />
                    {item}
                  </label>
                ))}
              </div>
            </div>

            <div className="space-y-2">
              <Label>{t("openclaw.teProvider.reasoningEfforts", { defaultValue: "推理档位" })}</Label>
              <div className="flex flex-wrap gap-3">
                {REASONING_EFFORTS.map((item) => (
                  <label key={item} className="flex items-center gap-2 text-xs">
                    <Checkbox
                      data-testid={`te-model-reasoning-${item}`}
                      checked={(model.reasoningEfforts ?? []).includes(item)}
                      onCheckedChange={(checked) =>
                        updateModel(index, {
                          reasoningEfforts: toggleList(
                            model.reasoningEfforts,
                            item,
                            checked === true,
                          ),
                        })
                      }
                    />
                    {item}
                  </label>
                ))}
              </div>
            </div>

            <div className="flex flex-wrap gap-4">
              <label className="flex items-center gap-2 text-xs">
                <Checkbox
                  data-testid="te-model-supports-tools"
                  checked={model.supportsTools === true}
                  onCheckedChange={(checked) =>
                    updateModel(index, { supportsTools: checked === true })
                  }
                />
                {t("openclaw.teProvider.supportsTools", { defaultValue: "支持工具调用" })}
              </label>
              <label className="flex items-center gap-2 text-xs">
                <Checkbox
                  data-testid="te-model-supports-reasoning"
                  checked={model.supportsReasoning === true}
                  onCheckedChange={(checked) =>
                    updateModel(index, { supportsReasoning: checked === true })
                  }
                />
                {t("openclaw.teProvider.supportsReasoning", { defaultValue: "支持推理" })}
              </label>
            </div>
          </div>
        ))}
      </div>

      {errors.length > 0 && (
        <div
          className="rounded-lg border border-destructive/40 bg-destructive/5 p-3"
          role="alert"
          data-testid="te-provider-errors"
        >
          <p className="text-xs font-medium">
            {t("openclaw.teProvider.invalid", {
              defaultValue: "当前设置还不能写入 Agent 配置",
            })}
          </p>
          <ul className="mt-1 list-disc pl-4 text-xs">
            {errors.map((code) => (
              <li key={code}>{errorLabel(code)}</li>
            ))}
          </ul>
        </div>
      )}

      <div className="rounded-lg border bg-muted/30 p-4 space-y-2">
        <h3 className="text-sm font-medium">
          {t("openclaw.teProvider.runtimeFields", {
            defaultValue: "由运行时注入、不落盘的字段",
          })}
        </h3>
        <ul className="space-y-1 text-xs text-muted-foreground">
          {runtimeFields.map((field) => (
            <li key={field.field} data-testid="te-runtime-field">
              <span className="font-mono">{field.field}</span>
              {" · "}
              {field.filledBy}
              {" · "}
              {field.note}
            </li>
          ))}
        </ul>
      </div>

      <div className="rounded-lg border p-4 space-y-3" data-testid="te-runtime-status">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-sm font-medium">
              {t("openclaw.teProvider.runtimeStatus", {
                defaultValue: "运行态（只读探针）",
              })}
            </h3>
            <p className="text-xs text-muted-foreground">
              {t("openclaw.teProvider.runtimeStatusHint", {
                defaultValue:
                  "只查询注入器存活与上游在线状态；不会读取、显示或落盘 Task、lease、session 与 binding。",
              })}
            </p>
          </div>
          <button
            type="button"
            className="text-xs underline"
            data-testid="te-refresh-runtime"
            disabled={runtimeBusy}
            onClick={() => void refreshRuntimeStatus()}
          >
            {runtimeBusy
              ? t("openclaw.teProvider.runtimeChecking", { defaultValue: "检查中…" })
              : t("openclaw.teProvider.runtimeRefresh", { defaultValue: "刷新运行态" })}
          </button>
        </div>

        {runtimeError ? (
          <p className="text-xs text-destructive" data-testid="te-runtime-error">
            {runtimeError}
          </p>
        ) : null}

        {runtimeStatus ? (
          <ul className="space-y-1 text-xs" data-testid="te-runtime-summary">
            <li>
              注入器：
              {runtimeStatus.sidecarReachable
                ? `存活（${runtimeStatus.sidecarStatus ?? "ok"}，${runtimeStatus.latencyMs}ms）`
                : `不可达（${runtimeStatus.sidecarError ?? "connect_failed"}）`}
            </li>
            <li>{providerStatusText(runtimeStatus)}</li>
            <li>
              检查时间：{runtimeStatus.checkedAt}
              {runtimeStatus.provider?.checkedAt
                ? ` · 探针时间 ${runtimeStatus.provider.checkedAt}`
                : ""}
            </li>
            <li data-testid="te-runtime-binding-note">
              运行时绑定：{runtimeStatus.runtimeBindingExposed ? "可读" : "不通过 HTTP 暴露（按设计）"}
            </li>
          </ul>
        ) : null}
      </div>
    </div>
  );
}
