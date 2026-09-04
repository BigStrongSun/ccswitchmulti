import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CodexCompatibilitySummary } from "./CodexCompatibilitySummary";

describe("CodexCompatibilitySummary", () => {
  it("does not classify unavailable responses as missing reasoning", () => {
    render(
      <CodexCompatibilitySummary
        transport="open_ai_responses"
        readiness="unverified"
        baselinePassed={false}
        reasoningSemantic="none"
        reasoningSource="none"
        toolSchemaDialect="openai"
        historyReplay="native_only"
        retries={[]}
        running={false}
        selected={false}
      />,
    );
    expect(
      screen.queryByText(/上游没有可展示的推理正文/),
    ).not.toBeInTheDocument();
  });
  it("does not call native opaque reasoning repaired or invent visible thinking", () => {
    render(
      <CodexCompatibilitySummary
        transport="open_ai_responses"
        readiness="verified"
        baselinePassed
        reasoningSemantic="opaque"
        reasoningSource="native_responses"
        toolSchemaDialect="openai"
        historyReplay="native_only"
        retries={[]}
        running={false}
        selected
      />,
    );
    expect(screen.getByText("本次检查通过")).toBeInTheDocument();
    expect(screen.queryByText("适配后通过")).not.toBeInTheDocument();
    expect(
      screen.getByText(/不会生成、解密或伪造思考内容/),
    ).toBeInTheDocument();
    expect(screen.getByText(/不是当前配置已生效的凭证/)).toBeInTheDocument();
  });
  it("does not infer a repair from an in-flight baseline request", () => {
    render(
      <CodexCompatibilitySummary
        transport="open_ai_chat"
        readiness={null}
        baselinePassed={false}
        reasoningSemantic={null}
        reasoningSource={null}
        toolSchemaDialect={null}
        historyReplay={null}
        retries={[]}
        running
        selected={false}
      />,
    );
    expect(screen.getByText("正在检测响应与协议差异")).toBeInTheDocument();
    expect(screen.queryByText("正在自动适配并重试")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/上游使用 Chat 响应结构/),
    ).not.toBeInTheDocument();
  });
});
