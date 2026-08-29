import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  CodexApiFormat,
  CodexHistoryReplay,
  CodexProtocolMode,
  CodexReasoningProjection,
  CodexToolSchemaDialect,
} from "@/types";

interface CodexProtocolAdvancedSettingsProps {
  mode: CodexProtocolMode;
  apiFormat: CodexApiFormat;
  reasoningProjection: CodexReasoningProjection;
  toolSchemaDialect: CodexToolSchemaDialect;
  historyReplay: CodexHistoryReplay;
  onModeChange: (value: CodexProtocolMode) => void;
  onApiFormatChange: (value: CodexApiFormat) => void;
  onReasoningProjectionChange: (value: CodexReasoningProjection) => void;
  onToolSchemaDialectChange: (value: CodexToolSchemaDialect) => void;
  onHistoryReplayChange: (value: CodexHistoryReplay) => void;
}

export function CodexProtocolAdvancedSettings({
  mode,
  apiFormat,
  reasoningProjection,
  toolSchemaDialect,
  historyReplay,
  onModeChange,
  onApiFormatChange,
  onReasoningProjectionChange,
  onToolSchemaDialectChange,
  onHistoryReplayChange,
}: CodexProtocolAdvancedSettingsProps) {
  return (
    <div className="space-y-3 rounded-md border border-border-default p-3">
      <div className="space-y-1.5">
        <Label>协议配置方式</Label>
        <Select value={mode} onValueChange={onModeChange}>
          <SelectTrigger aria-label="协议配置方式">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="auto">自动探测（推荐）</SelectItem>
            <SelectItem value="manual">手动覆盖（高级）</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {mode === "auto" ? (
        <p className="text-xs leading-relaxed text-muted-foreground">
          保存前会自动测试 Responses 与 Chat、流式响应、工具调用和历史续轮；
          “自动推荐”来自探测证据，最终保存时采用能够完成真实 Codex
          工作流的协议。
        </p>
      ) : (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label>最终使用协议</Label>
            <Select
              value={apiFormat}
              onValueChange={(value) =>
                onApiFormatChange(value as CodexApiFormat)
              }
            >
              <SelectTrigger aria-label="最终使用协议">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="openai_chat">Chat Completions</SelectItem>
                <SelectItem value="openai_responses">Responses</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs leading-relaxed text-muted-foreground">
              这里仅覆盖最终保存和运行时使用的协议，不会改写或删除 Responses /
              Chat 的独立探测证据与自动推荐。
            </p>
          </div>

          <div className="space-y-1.5">
            <Label>工具 Schema</Label>
            <Select
              value={toolSchemaDialect}
              onValueChange={onToolSchemaDialectChange}
            >
              <SelectTrigger aria-label="工具 Schema">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="openai">OpenAI JSON Schema</SelectItem>
                <SelectItem value="moonshot_mfjs">
                  Moonshot MFJS（Kimi）
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          {apiFormat === "openai_chat" ? (
            <>
              <div className="space-y-1.5">
                <Label>Chat 推理展示</Label>
                <Select
                  value={reasoningProjection}
                  onValueChange={onReasoningProjectionChange}
                >
                  <SelectTrigger aria-label="Chat 推理展示">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="raw_reasoning_text">
                      原始推理正文
                    </SelectItem>
                    <SelectItem value="none">不展示推理</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <p className="text-xs text-muted-foreground">
                Chat 历史续轮固定写入
                reasoning_content，避免把展示字段与续轮字段混用。
              </p>
            </>
          ) : (
            <div className="space-y-1.5">
              <Label>Responses 历史续轮</Label>
              <Select
                value={historyReplay}
                onValueChange={onHistoryReplayChange}
              >
                <SelectTrigger aria-label="Responses 历史续轮">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="native_only">原生 Responses</SelectItem>
                  <SelectItem value="responses_reasoning_text_content">
                    reasoning_text content 兼容
                  </SelectItem>
                  <SelectItem value="omit">不回放推理项</SelectItem>
                </SelectContent>
              </Select>
            </div>
          )}

          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs leading-relaxed text-amber-900 dark:text-amber-200">
            已完成的自动探测会继续保留；如果没有对应协议的 Verified
            证据，手动覆盖可能造成 HTTP
            400/422、工具续轮失败，或推理内容不可见。
          </div>
        </div>
      )}
    </div>
  );
}
