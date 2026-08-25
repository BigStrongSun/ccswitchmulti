import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CodexProtocolProbeProgressDialog } from "@/components/providers/forms/CodexProtocolProbeProgressDialog";
import type { CodexProtocolProbeProgressEvent } from "@/lib/api/protocol-compatibility";

describe("CodexProtocolProbeProgressDialog", () => {
  it("shows the complete model denominator before later models start", () => {
    render(
      <CodexProtocolProbeProgressDialog
        open
        running
        expectedModels={["qwen3.8", "deepseek-v4"]}
        events={[
          {
            kind: "candidate_started",
            model: "qwen3.8",
          },
        ]}
        outcome={null}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(screen.getByText(/正在验证模型 0\/2/)).toBeInTheDocument();
    expect(
      screen.getByRole("article", { name: "deepseek-v4 探测进度" }),
    ).toHaveTextContent("等待开始");
  });

  it("shows each model, transport, and deep-probe stage while probing", () => {
    const events: CodexProtocolProbeProgressEvent[] = [
      {
        kind: "candidate_started",
        model: "qwen3.8",
      },
      {
        kind: "stage_finished",
        model: "qwen3.8",
        transport: "open_ai_responses",
        stage: "baseline",
        stageStatus: "passed",
      },
      {
        kind: "stage_started",
        model: "qwen3.8",
        transport: "open_ai_responses",
        stage: "streaming",
      },
      {
        kind: "reasoning_classified",
        model: "qwen3.8",
        transport: "open_ai_responses",
        stage: "reasoning",
        reasoningSemantic: "summary",
        reasoningSource: "native_responses",
      },
    ];

    render(
      <CodexProtocolProbeProgressDialog
        open
        running
        events={events}
        outcome={null}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Codex 兼容性深度探测" }),
    ).toBeInTheDocument();
    const modelCard = screen.getByRole("article", { name: "qwen3.8 探测进度" });
    expect(within(modelCard).getByText("Responses")).toBeInTheDocument();
    expect(within(modelCard).getByText("基础响应")).toBeInTheDocument();
    expect(within(modelCard).getByText("流式 SSE")).toBeInTheDocument();
    expect(within(modelCard).getByText("思考内容")).toBeInTheDocument();
    expect(within(modelCard).getByText("工具调用")).toBeInTheDocument();
    expect(within(modelCard).getByText("工具续轮")).toBeInTheDocument();
    expect(within(modelCard).getByText("摘要")).toBeInTheDocument();
    expect(within(modelCard).getByText("检测中")).toBeInTheDocument();
  });
});
