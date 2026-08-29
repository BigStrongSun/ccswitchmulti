import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type {
  CodexProtocolProbeProgressEvent,
  CodexProviderProtocolPreflightOutcome,
} from "@/lib/api/protocol-compatibility";
import { CodexProtocolProbeProgressDialog } from "./CodexProtocolProbeProgressDialog";

describe("CodexProtocolProbeProgressDialog", () => {
  it("shows the verified tool schema and history replay strategy for each branch", () => {
    const outcome = {
      provider: {
        id: "provider",
        name: "Kimi",
        settingsConfig: {},
      },
      receiptIds: ["receipt-kimi-k2"],
      protocolApplied: true,
      observations: [],
      records: [
        {
          probeVersion: 3,
          target: {
            provider_id: "provider",
            route_id: null,
            public_model: "kimi-k2",
            upstream_model: "kimi-k2",
            transport: "open_ai_responses",
            endpoint_fingerprint: "endpoint",
            authentication_kind: "bearer",
            credential_fingerprint: "credential",
            request_policy_fingerprint: "policy",
          },
          result: {
            selected_transport: "open_ai_responses",
            readiness: "verified",
            branches: [
              {
                assessment: {
                  transport: "open_ai_responses",
                  baseline: "passed",
                  streaming: "passed",
                  forced_tool: "passed",
                  continuation: "passed",
                },
                reasoning_shape: {
                  semantic: "summary",
                  source: "native_responses",
                  pre_tool_visible_content: "absent",
                },
                tool_schema_dialect: "moonshot_mfjs",
                history_replay: "responses_reasoning_text_content",
                failures: [],
              },
              {
                assessment: {
                  transport: "open_ai_chat",
                  baseline: "passed",
                  streaming: "passed",
                  forced_tool: "passed",
                  continuation: "passed",
                },
                reasoning_shape: {
                  semantic: "readable",
                  source: "reasoning_content",
                  pre_tool_visible_content: "absent",
                },
                tool_schema_dialect: "moonshot_mfjs",
                history_replay: "chat_reasoning_content",
                failures: [],
              },
            ],
          },
          testedAt: 1,
          expiresAt: 2,
        },
      ],
    } as CodexProviderProtocolPreflightOutcome;

    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["kimi-k2"]}
        events={[]}
        outcome={outcome}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(screen.getAllByText("工具 Schema：Moonshot MFJS")).toHaveLength(2);
    expect(
      screen.getByText("历史续轮：reasoning_text content"),
    ).toBeInTheDocument();
    expect(screen.getByText("历史续轮：reasoning_content")).toBeInTheDocument();
    expect(screen.getByText("上游原生摘要")).toBeInTheDocument();
    expect(screen.getByText("原始推理正文")).toBeInTheDocument();
  });

  it("marks opaque reasoning as unavailable for Codex presentation", () => {
    const outcome = {
      provider: {
        id: "provider",
        name: "Opaque gateway",
        settingsConfig: {},
      },
      receiptIds: ["receipt-opaque-model"],
      protocolApplied: false,
      observations: [],
      records: [
        {
          probeVersion: 6,
          target: {
            provider_id: "provider",
            route_id: null,
            public_model: "opaque-model",
            upstream_model: "opaque-model",
            transport: "open_ai_responses",
            endpoint_fingerprint: "endpoint",
            authentication_kind: "bearer",
            credential_fingerprint: "credential",
            request_policy_fingerprint: "policy",
          },
          result: {
            selected_transport: "open_ai_responses",
            readiness: "verified",
            branches: [
              {
                assessment: {
                  transport: "open_ai_responses",
                  baseline: "passed",
                  streaming: "passed",
                  forced_tool: "passed",
                  continuation: "passed",
                },
                reasoning_shape: {
                  semantic: "opaque",
                  source: "native_responses",
                  pre_tool_visible_content: "absent",
                },
                tool_schema_dialect: "openai",
                history_replay: "native_only",
                failures: [],
              },
            ],
          },
          testedAt: 1,
          expiresAt: 2,
        },
      ],
    } as CodexProviderProtocolPreflightOutcome;

    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["opaque-model"]}
        events={[]}
        outcome={outcome}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    const responses = screen.getByText("Responses").closest("section");
    expect(responses).not.toBeNull();
    const reasoningRow = within(responses as HTMLElement)
      .getByText("思考内容")
      .closest("div");
    expect(reasoningRow).not.toBeNull();
    expect(
      within(reasoningRow as HTMLElement).getByText(
        "加密/不透明（Codex 无法展示）",
      ),
    ).toBeInTheDocument();
    expect(
      within(reasoningRow as HTMLElement).getByText("不支持"),
    ).toBeInTheDocument();
  });

  it("renders live stage progress and prevents closing while a probe is running", () => {
    const onOpenChange = vi.fn();
    const events = [
      {
        kind: "stage_started",
        model: "qwen3.8",
        transport: "open_ai_responses",
        stage: "baseline",
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
    ] as CodexProtocolProbeProgressEvent[];

    render(
      <CodexProtocolProbeProgressDialog
        open
        running
        expectedModels={["qwen3.8"]}
        events={events}
        outcome={null}
        error=""
        onOpenChange={onOpenChange}
      />,
    );

    expect(screen.getByText(/正在验证模型 0\/1/)).toBeInTheDocument();
    expect(screen.getByText("检测中")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "探测进行中" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "重新探测" })).toBeNull();
  });

  it("shows unavailable as not detected and offers an explicit retry", () => {
    const onRetry = vi.fn();
    const events = [
      {
        kind: "stage_finished",
        model: "qwen3.8",
        transport: "open_ai_responses",
        stage: "baseline",
        stageStatus: "failed",
        failure: {
          stage: "baseline",
          kind: "http_status",
          status_code: 521,
        },
      },
      {
        kind: "reasoning_classified",
        model: "qwen3.8",
        transport: "open_ai_responses",
        reasoningSemantic: "none",
        reasoningSource: "none",
      },
    ] as CodexProtocolProbeProgressEvent[];

    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["qwen3.8"]}
        events={events}
        outcome={null}
        error=""
        onOpenChange={vi.fn()}
        onRetry={onRetry}
      />,
    );

    expect(screen.getByText("HTTP 521 · 上游不可达")).toBeInTheDocument();
    expect(screen.getByText("未检测")).toBeInTheDocument();
    expect(screen.getByText("已跳过")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重新探测" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it.each([
    [401, "HTTP 401 · 认证失败"],
    [403, "HTTP 403 · 当前凭据无权限"],
    [429, "HTTP 429 · 限流或额度不足"],
    [500, "HTTP 500 · 上游服务异常"],
  ])(
    "explains HTTP %s without treating it as protocol support",
    (status, label) => {
      const events = [
        {
          kind: "stage_finished",
          model: "provider-model",
          transport: "open_ai_chat",
          stage: "baseline",
          stageStatus: "failed",
          failure: {
            stage: "baseline",
            kind: "http_status",
            status_code: status,
          },
        },
      ] as CodexProtocolProbeProgressEvent[];

      render(
        <CodexProtocolProbeProgressDialog
          open
          running={false}
          expectedModels={["provider-model"]}
          events={events}
          outcome={null}
          error=""
          onOpenChange={vi.fn()}
        />,
      );

      expect(screen.getByText(label)).toBeInTheDocument();
    },
  );
});
