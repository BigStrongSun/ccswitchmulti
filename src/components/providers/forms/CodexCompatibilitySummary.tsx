import type {
  CodexCompatibilityRule,
  CodexProtocolProbeReadiness,
  CodexProtocolTransport,
  CodexReasoningSemantic,
  CodexReasoningSource,
} from "@/lib/api/protocol-compatibility";
import type { CodexHistoryReplay, CodexToolSchemaDialect } from "@/types";

interface Props {
  transport: CodexProtocolTransport;
  readiness: CodexProtocolProbeReadiness | null;
  baselinePassed: boolean;
  reasoningSemantic: CodexReasoningSemantic | null;
  reasoningSource: CodexReasoningSource | null;
  toolSchemaDialect: CodexToolSchemaDialect | null;
  historyReplay: CodexHistoryReplay | null;
  retries: CodexCompatibilityRule[];
  running: boolean;
  selected: boolean;
}

export function CodexCompatibilitySummary(props: Props) {
  const rows: string[] = [];
  const rules = new Set(props.retries);
  if (props.toolSchemaDialect === "moonshot_mfjs") rules.add("tool_schema");
  if (props.historyReplay === "responses_reasoning_text_content")
    rules.add("reasoning_text_replay");
  if (props.historyReplay === "omit") rules.add("omit_reasoning");
  if (props.transport === "open_ai_chat" && props.baselinePassed) {
    rows.push(
      "上游使用 Chat 响应结构，Codex 使用 Responses：CCSM 转换正文、工具调用和流式事件，不改写模型回答的含义。",
    );
    if (
      props.reasoningSource &&
      props.reasoningSource !== "none" &&
      props.reasoningSource !== "native_responses"
    ) {
      rows.push(
        `上游返回 ${props.reasoningSource}：CCSM 读取该推理来源，${props.reasoningSemantic === "readable" ? "映射为 Codex 可读推理事件" : props.reasoningSemantic === "summary" ? "映射为推理摘要事件" : "不生成可见推理正文"}。`,
      );
    }
  }
  if (rules.has("tool_schema"))
    rows.push(
      "默认工具请求被拒绝或未返回有效工具调用：CCSM 使用兼容工具 Schema（Moonshot MFJS）重新测试；这是请求结构适配，不是修改响应正文，也不单凭 HTTP 错误断言上游不支持工具。",
    );
  if (rules.has("reasoning_text_replay"))
    rows.push(
      "原生推理历史结构不被接受：CCSM 将可读推理重建为 reasoning_text content，再验证工具续轮。",
    );
  if (rules.has("omit_reasoning"))
    rows.push(
      "推理历史仍不被接受：CCSM 仅移除推理项，保留工具调用和工具结果；该降级会减少回传给模型的推理历史。",
    );
  const hasAdaptation = rows.length > 0;
  const title =
    props.readiness === "verified"
      ? hasAdaptation
        ? "适配后通过"
        : "本次检查通过"
      : props.readiness !== null
        ? hasAdaptation
          ? "适配仍未通过"
          : "尚未通过兼容验证"
        : props.running
          ? props.retries.length > 0
            ? "正在自动适配并重试"
            : "正在检测响应与协议差异"
          : "验证未完成";
  return (
    <div
      className="space-y-2 rounded-md border border-sky-500/25 bg-sky-500/5 p-3 text-xs leading-relaxed"
      aria-label="自动兼容处理"
    >
      <p className="font-medium text-foreground" role="status">
        {title}
      </p>
      {rows.length > 0 ? (
        <ul className="list-disc space-y-1 pl-4">
          {rows.map((row) => (
            <li key={row}>{row}</li>
          ))}
        </ul>
      ) : (
        <p className="text-muted-foreground">
          {props.readiness === "verified"
            ? "未记录额外兼容策略；不代表所有响应字段都与官方完全一致。"
            : "还没有可确认的自动修复结果，不能把检测中或请求失败视为已修复。"}
        </p>
      )}
      {props.baselinePassed &&
        (props.reasoningSemantic === "opaque" ||
          props.reasoningSemantic === "none") && (
          <p className="text-amber-700 dark:text-amber-300">
            上游没有可展示的推理正文；CCSM 不会生成、解密或伪造思考内容。
          </p>
        )}
      {props.readiness === "verified" && (
        <p>
          {props.selected
            ? "该分支被自动推荐。"
            : "这是备用分支的验证结果，不代表最终已选用。"}
        </p>
      )}
      <p className="border-t pt-2 text-muted-foreground">
        以上是探测证据，不是当前配置已生效的凭证。探测不保存；保存并启用后，运行请求才按最终配置适配。手动覆盖不等于验证通过。
      </p>
    </div>
  );
}
