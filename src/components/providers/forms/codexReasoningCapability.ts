import type { CodexReasoningEffort } from "@/types";

/**
 * 为 Provider 原生档位补齐同名映射。额外的兼容映射会被保留；用户只需
 * 编辑名称不同的部分，落库前仍得到完整、显式的转换契约。
 */
export function completeCodexReasoningEffortMap({
  supportedEfforts,
  effortMap,
}: {
  supportedEfforts: CodexReasoningEffort[];
  effortMap?: Partial<Record<CodexReasoningEffort, CodexReasoningEffort>>;
}): Partial<Record<CodexReasoningEffort, CodexReasoningEffort>> {
  const completed = { ...effortMap };
  for (const effort of supportedEfforts) {
    completed[effort] ??= effort;
  }
  return completed;
}
