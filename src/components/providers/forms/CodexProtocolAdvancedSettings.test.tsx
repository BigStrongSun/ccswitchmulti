import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CodexProtocolAdvancedSettings } from "./CodexProtocolAdvancedSettings";

describe("CodexProtocolAdvancedSettings", () => {
  it("keeps ordinary users on automatic probing without exposing manual knobs", () => {
    render(
      <CodexProtocolAdvancedSettings
        mode="auto"
        apiFormat="openai_responses"
        reasoningProjection="none"
        toolSchemaDialect="openai"
        historyReplay="native_only"
        onModeChange={vi.fn()}
        onApiFormatChange={vi.fn()}
        onReasoningProjectionChange={vi.fn()}
        onToolSchemaDialectChange={vi.fn()}
        onHistoryReplayChange={vi.fn()}
      />,
    );

    expect(screen.getByText(/自动测试 Responses 与 Chat/)).toBeInTheDocument();
    expect(screen.queryByText("工具 Schema")).not.toBeInTheDocument();
    expect(screen.queryByText("Responses 历史续轮")).not.toBeInTheDocument();
  });

  it("shows protocol-specific manual controls and an explicit risk warning", () => {
    const onApiFormatChange = vi.fn();
    const { rerender } = render(
      <CodexProtocolAdvancedSettings
        mode="manual"
        apiFormat="openai_chat"
        reasoningProjection="raw_reasoning_text"
        toolSchemaDialect="moonshot_mfjs"
        historyReplay="omit"
        onModeChange={vi.fn()}
        onApiFormatChange={onApiFormatChange}
        onReasoningProjectionChange={vi.fn()}
        onToolSchemaDialectChange={vi.fn()}
        onHistoryReplayChange={vi.fn()}
      />,
    );

    expect(screen.getByText("工具 Schema")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("combobox", { name: "最终使用协议" }));
    fireEvent.click(screen.getByText("Responses"));
    expect(onApiFormatChange).toHaveBeenCalledWith("openai_responses");
    expect(screen.getByText("Chat 推理展示")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("combobox", { name: "Chat 推理展示" }));
    expect(screen.queryByText("上游原生摘要")).not.toBeInTheDocument();
    expect(screen.getByText(/固定写入 reasoning_content/)).toBeInTheDocument();
    expect(screen.getByText(/可能造成 HTTP 400/)).toBeInTheDocument();

    rerender(
      <CodexProtocolAdvancedSettings
        mode="manual"
        apiFormat="openai_responses"
        reasoningProjection="reasoning_summary"
        toolSchemaDialect="openai"
        historyReplay="responses_reasoning_text_content"
        onModeChange={vi.fn()}
        onApiFormatChange={vi.fn()}
        onReasoningProjectionChange={vi.fn()}
        onToolSchemaDialectChange={vi.fn()}
        onHistoryReplayChange={vi.fn()}
      />,
    );
    expect(screen.getByText("Responses 历史续轮")).toBeInTheDocument();
    expect(screen.queryByText("Chat 推理展示")).not.toBeInTheDocument();
  });
});
