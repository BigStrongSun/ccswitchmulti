import type { Provider } from "@/types";
import { readCodexModelCatalog } from "@/utils/codexSpawnAgentCandidates";

/**
 * 后端保存门禁的稳定错误码。
 *
 * 保存时必须保留这个 token：Provider 表单、向导和测试都靠它识别“已启用且可路由的
 * 角色没有声明推理能力”这一类阻塞，而不是靠中文文案。
 */
export const UNKNOWN_REASONING_CAPABILITY_CODE =
  "unknown_reasoning_capability_requires_declaration";

export interface UnknownReasoningBlocker {
  models: string[];
  message: string;
}

/**
 * 从保存门禁错误里取出被阻塞的模型名。
 *
 * 后端把模型名放在错误码之后（`... (code): model-a, model-b`）。旧消息只有错误码，
 * 此时返回空数组；调用方仍应展示通用说明，而不是把这条诊断丢掉。
 */
export function readUnknownReasoningModels(message: string): string[] {
  const codeIndex = message.indexOf(UNKNOWN_REASONING_CAPABILITY_CODE);
  if (codeIndex < 0) return [];
  const tail = message.slice(
    codeIndex + UNKNOWN_REASONING_CAPABILITY_CODE.length,
  );
  const list = tail.replace(/^[)\s:：]+/, "").trim();
  if (!list) return [];
  const seen = new Set<string>();
  const models: string[] = [];
  for (const raw of list.split(/[,，、]/)) {
    const model = raw.trim();
    if (!model) continue;
    const key = model.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    models.push(model);
  }
  return models;
}

export function isUnknownReasoningCapabilityError(message: string): boolean {
  return message.includes(UNKNOWN_REASONING_CAPABILITY_CODE);
}

/**
 * 把保存门禁错误转换成向导可展示的阻塞对象；不是这一类错误时返回 null，
 * 调用方继续走原有的通用错误路径。
 */
export function readUnknownReasoningBlocker(
  message: string,
): UnknownReasoningBlocker | null {
  if (!isUnknownReasoningCapabilityError(message)) return null;
  return { models: readUnknownReasoningModels(message), message };
}

/**
 * 找到声明该模型的模型源，供“去声明推理能力”按钮直接打开对应 Provider。
 *
 * 只匹配 catalog 的 `model` 字段：这里的目标是让用户回到能声明能力的那一页，
 * 不是重新实现后端的路由/别名判定。
 */
export function findModelSource(
  providers: Provider[],
  model: string,
): Provider | undefined {
  const target = model.trim().toLowerCase();
  if (!target) return undefined;
  return providers.find((provider) =>
    readCodexModelCatalog(provider).models.some(
      (entry) => entry.model?.trim().toLowerCase() === target,
    ),
  );
}

/** 阻塞说明的正文；模型名缺失时给出可执行的下一步，而不是只回显错误码。 */
export function unknownReasoningBlockerDetail(models: string[]): string {
  if (models.length > 0) {
    return `以下已启用的 Sub-Agent 角色使用的模型没有声明推理能力：${models.join("、")}。可以先禁用这些角色再保存，或到对应 Provider 的模型目录声明能力后重试。`;
  }
  return "存在已启用的 Sub-Agent 角色使用的模型没有声明推理能力。请到对应 Provider 的模型目录声明能力，或先禁用这些角色后再保存。";
}
