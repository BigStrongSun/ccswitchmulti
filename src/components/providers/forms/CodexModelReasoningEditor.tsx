import type {
  CodexModelReasoningCapability,
  CodexReasoningControlKind,
  CodexReasoningEffort,
} from "@/types";

const PROVIDER_EFFORTS: CodexReasoningEffort[] = [
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
];

function identityCompleteMap(
  capability: CodexModelReasoningCapability,
): Partial<Record<CodexReasoningEffort, CodexReasoningEffort>> {
  const completed = { ...capability.upstream.effortMap };
  for (const effort of capability.supportedEfforts) {
    completed[effort] ??= effort;
  }
  return completed;
}

export interface CodexModelReasoningEditorProps {
  model: string;
  capability: CodexModelReasoningCapability;
  readOnly: boolean;
  onChange: (capability: CodexModelReasoningCapability) => void;
}

/**
 * 模型级推理能力的结构化编辑器。Provider 原生能力、上游 wire 契约与
 * Codex 映射分组展示，避免用户把三个层面的档位误当成同一字段。
 */
export function CodexModelReasoningEditor({
  model,
  capability,
  readOnly,
  onChange,
}: CodexModelReasoningEditorProps) {
  const isGraded =
    (capability.controlKind ??
      (capability.supportedEfforts.length ? "graded" : "unknown")) === "graded";
  const completedMap = identityCompleteMap(capability);
  const addableEfforts = PROVIDER_EFFORTS.filter(
    (effort) => !capability.supportedEfforts.includes(effort),
  );

  const update = (next: CodexModelReasoningCapability) =>
    onChange({ ...next, schemaVersion: 2, source: "user" });

  const updateControlKind = (controlKind: CodexReasoningControlKind) => {
    const graded = controlKind === "graded";
    const boolean = controlKind === "boolean";
    update({
      ...capability,
      supportStatus:
        controlKind === "none"
          ? "confirmed_unsupported"
          : "confirmed_supported",
      controlKind,
      supportedEfforts: graded ? capability.supportedEfforts : [],
      defaultEffort: graded ? capability.defaultEffort : undefined,
      disableAllowed: boolean ? capability.disableAllowed : false,
      upstream: graded
        ? {
            ...capability.upstream,
            format:
              capability.upstream.format === "string" ||
              capability.upstream.format === "reasoning_object"
                ? capability.upstream.format
                : "string",
            parameter:
              capability.upstream.parameter === "reasoning.effort"
                ? "reasoning.effort"
                : "reasoning_effort",
            effortMap: completedMap,
          }
        : boolean
          ? {
              format: "boolean",
              parameter: "enable_thinking",
              effortMap: {},
            }
          : { format: "none", parameter: "none", effortMap: {} },
    });
  };

  return (
    <div className="space-y-4 text-xs">
      <label className="grid gap-1">
        <span className="font-medium">控制方式</span>
        <select
          className="rounded-md border bg-background px-3 py-2"
          aria-label={`${model} 推理控制方式`}
          value={capability.controlKind ?? (isGraded ? "graded" : "unknown")}
          disabled={readOnly}
          onChange={(event) =>
            updateControlKind(event.target.value as CodexReasoningControlKind)
          }
        >
          <option value="graded">分档推理（effort）</option>
          <option value="boolean">仅开关推理</option>
          <option value="budget">推理 token 预算</option>
          <option value="none">不支持推理</option>
          <option value="unknown">尚未确认</option>
        </select>
        <span className="text-muted-foreground">
          根据 Provider 对该模型公开的真实控制方式选择；不确定时保持“尚未确认”。
        </span>
      </label>

      {isGraded ? (
        <>
          <fieldset className="space-y-2 rounded-md border p-3">
            <legend className="px-1 font-medium">Provider 原生能力</legend>
            <p className="text-muted-foreground">
              只勾选该 Provider API 对当前模型实际接受的档位，不是 Codex
              的通用档位全集。
            </p>
            <div className="flex flex-wrap gap-3">
              {capability.supportedEfforts.map((effort) => (
                <label key={effort} className="flex items-center gap-1">
                  <input
                    type="checkbox"
                    aria-label={`Provider 原生档位 ${effort}`}
                    checked
                    disabled={readOnly}
                    onChange={() => {
                      const supportedEfforts =
                        capability.supportedEfforts.filter(
                          (candidate) => candidate !== effort,
                        );
                      const effortMap = { ...capability.upstream.effortMap };
                      for (const [source, target] of Object.entries(
                        effortMap,
                      )) {
                        if (target === effort) {
                          delete effortMap[source as CodexReasoningEffort];
                        }
                      }
                      update({
                        ...capability,
                        supportedEfforts,
                        defaultEffort: supportedEfforts.includes(
                          capability.defaultEffort as CodexReasoningEffort,
                        )
                          ? capability.defaultEffort
                          : supportedEfforts[0],
                        upstream: { ...capability.upstream, effortMap },
                      });
                    }}
                  />
                  {effort}
                </label>
              ))}
            </div>
            {addableEfforts.length ? (
              <label className="grid max-w-xs gap-1">
                <span>添加 Provider 档位</span>
                <select
                  className="rounded-md border bg-background px-3 py-2"
                  aria-label="添加 Provider 原生档位"
                  value=""
                  disabled={readOnly}
                  onChange={(event) => {
                    const effort = event.target.value as CodexReasoningEffort;
                    if (!effort) return;
                    update({
                      ...capability,
                      supportedEfforts: [
                        ...capability.supportedEfforts,
                        effort,
                      ],
                      defaultEffort: capability.defaultEffort ?? effort,
                      upstream: {
                        ...capability.upstream,
                        effortMap: { ...completedMap, [effort]: effort },
                      },
                    });
                  }}
                >
                  <option value="">选择要添加的档位…</option>
                  {addableEfforts.map((effort) => (
                    <option key={effort} value={effort}>
                      {effort}
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
          </fieldset>

          <label className="grid gap-1">
            <span className="font-medium">Provider 默认档位</span>
            <select
              className="rounded-md border bg-background px-3 py-2"
              aria-label={`${model} Provider 默认档位`}
              value={capability.defaultEffort ?? ""}
              disabled={readOnly}
              onChange={(event) =>
                update({
                  ...capability,
                  defaultEffort:
                    (event.target.value as CodexReasoningEffort) || undefined,
                })
              }
            >
              <option value="">未声明</option>
              {capability.supportedEfforts.map((effort) => (
                <option key={effort} value={effort}>
                  {effort}
                </option>
              ))}
            </select>
            <span className="text-muted-foreground">
              Codex 没有显式指定强度时使用；必须属于上面勾选的原生档位。
            </span>
          </label>

          <fieldset className="space-y-2 rounded-md border p-3">
            <legend className="px-1 font-medium">上游传参</legend>
            <label className="grid gap-1">
              <span>参数形态</span>
              <select
                className="rounded-md border bg-background px-3 py-2"
                aria-label={`${model} 上游参数形态`}
                value={capability.upstream.format}
                disabled={readOnly}
                onChange={(event) => {
                  const format = event.target.value as
                    | "string"
                    | "reasoning_object";
                  update({
                    ...capability,
                    upstream: {
                      ...capability.upstream,
                      format,
                      parameter:
                        format === "reasoning_object"
                          ? "reasoning.effort"
                          : "reasoning_effort",
                      effortMap: completedMap,
                    },
                  });
                }}
              >
                <option value="string">顶层字符串参数</option>
                <option value="reasoning_object">reasoning 对象</option>
              </select>
            </label>
            <label className="grid gap-1">
              <span>上游参数名</span>
              <input
                className="rounded-md border bg-background px-3 py-2"
                aria-label={`${model} 上游参数名`}
                value={capability.upstream.parameter}
                readOnly={readOnly}
                onChange={(event) =>
                  update({
                    ...capability,
                    upstream: {
                      ...capability.upstream,
                      parameter: event.target
                        .value as CodexModelReasoningCapability["upstream"]["parameter"],
                      effortMap: completedMap,
                    },
                  })
                }
              />
              <span className="text-muted-foreground">
                例如：reasoning_effort（顶层字符串）或
                reasoning.effort（对象字段）。
              </span>
            </label>
          </fieldset>

          <fieldset
            className="space-y-2 rounded-md border p-3"
            aria-label={`${model} Codex 到 Provider 映射`}
          >
            <legend className="px-1 font-medium">Codex → Provider 映射</legend>
            <p className="text-muted-foreground">
              同名档位会自动生成恒等映射；只有名称不同或需要降档时才需要修改。
              保存时当前模型的每个可选档位都必须有有效目标。
            </p>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {capability.supportedEfforts.map((effort) => (
                <label
                  key={effort}
                  className="grid grid-cols-[1fr_auto_1fr] items-center gap-1"
                >
                  <span>{effort}</span>
                  <span>→</span>
                  <select
                    className="rounded border bg-background px-2 py-1"
                    aria-label={`${effort} 映射目标`}
                    value={completedMap[effort] ?? ""}
                    disabled={readOnly}
                    onChange={(event) =>
                      update({
                        ...capability,
                        upstream: {
                          ...capability.upstream,
                          effortMap: {
                            ...completedMap,
                            [effort]: event.target
                              .value as CodexReasoningEffort,
                          },
                        },
                      })
                    }
                  >
                    {capability.supportedEfforts.map((target) => (
                      <option key={target} value={target}>
                        {target}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            </div>
          </fieldset>
        </>
      ) : (
        <p className="rounded-md border bg-muted/30 p-3 text-muted-foreground">
          当前控制方式不使用 effort 档位，因此不需要填写 Codex → Provider 映射。
        </p>
      )}

      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={capability.disableAllowed}
          disabled={readOnly || capability.controlKind === "none"}
          onChange={(event) =>
            update({ ...capability, disableAllowed: event.target.checked })
          }
        />
        Provider 支持显式关闭推理
      </label>
    </div>
  );
}
