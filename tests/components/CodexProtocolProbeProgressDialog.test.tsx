import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CodexProtocolProbeProgressDialog } from "@/components/providers/forms/CodexProtocolProbeProgressDialog";
import type {
  CodexProtocolProbeBranch,
  CodexProtocolProbeFailure,
  CodexProtocolProbeProgressEvent,
  CodexProviderProtocolPreflightOutcome,
} from "@/lib/api/protocol-compatibility";

function probeOutcome(
  branches: CodexProtocolProbeBranch[],
): CodexProviderProtocolPreflightOutcome {
  return {
    provider: {
      id: "fixture-provider",
      name: "Fixture provider",
      settingsConfig: {},
    },
    adaptationPreview: {
      persistence: "single",
      status: "ready",
      effectiveTransport: "open_ai_chat",
      models: [],
    },
    records: [
      {
        probeVersion: 1,
        target: {
          provider_id: "fixture-provider",
          route_id: null,
          public_model: "qwen3.8",
          upstream_model: "qwen3.8",
          transport: "open_ai_chat",
          endpoint_fingerprint: "redacted",
          authentication_kind: "bearer",
          credential_fingerprint: "redacted",
          request_policy_fingerprint: "redacted-policy",
        },
        result: {
          selected_transport: null,
          readiness: "unverified",
          branches,
        },
        testedAt: 1,
        expiresAt: 2,
      },
    ],
    observations: [],
    receiptIds: ["receipt-fixture-model"],
    protocolApplied: false,
  };
}

function branch(
  transport: "open_ai_responses" | "open_ai_chat",
  baseline: "passed" | "failed",
  failures: CodexProtocolProbeFailure[] = [],
): CodexProtocolProbeBranch {
  return {
    assessment: {
      transport,
      baseline,
      streaming: "skipped",
      forced_tool: "skipped",
      continuation: "skipped",
    },
    reasoning_shape: {
      semantic: "none",
      source: "none",
      pre_tool_visible_content: "absent",
    },
    failures,
  };
}

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
    expect(within(modelCard).getByText("上游原生摘要")).toBeInTheDocument();
    expect(within(modelCard).getByText("检测中")).toBeInTheDocument();
  });

  it.each([
    { baseline: "failed" as const, expected: "已跳过", rejected: "不支持" },
    { baseline: "passed" as const, expected: "不支持", rejected: "已跳过" },
  ])(
    "maps reasoning with no content from a $baseline baseline to $expected",
    ({ baseline, expected, rejected }) => {
      render(
        <CodexProtocolProbeProgressDialog
          open
          running={false}
          events={[]}
          outcome={probeOutcome([
            branch("open_ai_responses", baseline),
            branch("open_ai_chat", baseline),
          ])}
          error=""
          onOpenChange={vi.fn()}
        />,
      );

      const modelCard = screen.getByRole("article", {
        name: "qwen3.8 探测进度",
      });
      const reasoningRows = within(modelCard).getAllByText("思考内容");
      for (const label of reasoningRows) {
        const row = label.parentElement;
        expect(row).toHaveTextContent(expected);
        expect(row).not.toHaveTextContent(rejected);
      }
    },
  );

  it("shows a redacted HTTP 521 upstream-unreachable failure", () => {
    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        events={[]}
        outcome={probeOutcome([
          branch("open_ai_responses", "failed", [
            {
              stage: "baseline",
              kind: "http_status",
              status_code: 521,
            },
          ]),
        ])}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    const modelCard = screen.getByRole("article", {
      name: "qwen3.8 探测进度",
    });
    expect(
      within(modelCard).getByText("HTTP 521 · 上游不可达"),
    ).toBeInTheDocument();
    expect(modelCard).not.toHaveTextContent("private upstream response body");
    expect(modelCard).not.toHaveTextContent("https://");
    expect(modelCard).not.toHaveTextContent("Bearer");
  });

  it("shows the redacted failure immediately from a progress event", () => {
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
        stageStatus: "failed",
        failure: {
          stage: "baseline",
          kind: "http_status",
          status_code: 521,
        },
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

    expect(screen.getByText("HTTP 521 · 上游不可达")).toBeInTheDocument();
  });
});
