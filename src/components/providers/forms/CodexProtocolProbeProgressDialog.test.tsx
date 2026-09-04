import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type {
  CodexProtocolProbeProgressEvent,
  CodexProviderProtocolPreflightOutcome,
} from "@/lib/api/protocol-compatibility";
import { CodexProtocolProbeProgressDialog } from "./CodexProtocolProbeProgressDialog";

describe("CodexProtocolProbeProgressDialog", () => {
  it("aggregates the completed models across every provider batch", () => {
    const events: CodexProtocolProbeProgressEvent[] = [
      {
        kind: "candidate_finished",
        model: "qwen3.8",
        selectedTransport: "open_ai_chat",
        readiness: "verified",
      },
      {
        kind: "batch_finished",
        total: 1,
        verified: 1,
        partial: 0,
        failed: 0,
      },
      {
        kind: "candidate_finished",
        model: "glm-5.3",
        selectedTransport: "open_ai_chat",
        readiness: "partial",
      },
      {
        kind: "batch_finished",
        total: 1,
        verified: 0,
        partial: 1,
        failed: 0,
      },
    ];

    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["qwen3.8", "glm-5.3"]}
        events={events}
        outcome={null}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(
      screen.getByText("已完成 2 个模型：Verified 1，Partial 1，Failed 0。"),
    ).toBeInTheDocument();
  });

  it("keeps duplicate public model names distinct across provider batches", () => {
    const expectedTargets = [
      {
        providerId: "kimi-responses",
        providerName: "Kimi Responses",
        model: "k3",
      },
      {
        providerId: "kimi-chat",
        providerName: "Kimi Chat",
        model: "k3",
      },
    ];
    const events: Array<
      CodexProtocolProbeProgressEvent & {
        providerId: string;
        providerName: string;
      }
    > = [
      {
        kind: "candidate_finished",
        providerId: "kimi-responses",
        providerName: "Kimi Responses",
        model: "k3",
        selectedTransport: "open_ai_chat",
        readiness: "verified",
      },
      {
        kind: "batch_finished",
        providerId: "kimi-responses",
        providerName: "Kimi Responses",
        total: 1,
        verified: 1,
        partial: 0,
        failed: 0,
      },
      {
        kind: "candidate_finished",
        providerId: "kimi-chat",
        providerName: "Kimi Chat",
        model: "k3",
        selectedTransport: "open_ai_chat",
        readiness: "verified",
      },
      {
        kind: "batch_finished",
        providerId: "kimi-chat",
        providerName: "Kimi Chat",
        total: 1,
        verified: 1,
        partial: 0,
        failed: 0,
      },
    ];

    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["k3", "k3"]}
        {...{ expectedTargets }}
        events={events}
        outcome={null}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(
      screen.getByText("已完成 2 个模型：Verified 2，Partial 0，Failed 0。"),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("article")).toHaveLength(2);
    expect(screen.getByText("Kimi Responses")).toBeInTheDocument();
    expect(screen.getByText("Kimi Chat")).toBeInTheDocument();
  });

  it("shows the verified tool schema and history replay strategy for each branch", () => {
    const outcome = {
      provider: {
        id: "provider",
        name: "Kimi",
        settingsConfig: {},
      },
      adaptationPreview: {
        persistence: "single",
        status: "ready",
        effectiveTransport: "open_ai_chat",
        models: [],
      },
      receiptIds: ["receipt-kimi-k2"],
      protocolApplied: true,
      probeUsage: {
        inputTokens: 1200,
        outputTokens: 300,
        cacheReadTokens: 200,
        cacheCreationTokens: 0,
        totalTokens: 1500,
        reportedResponses: 8,
        successfulResponses: 10,
        estimatedCostUsd: "0.0123",
        pricedModels: ["kimi-k2"],
        unpricedModels: [],
      },
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
    expect(screen.getAllByText("适配后通过")).toHaveLength(2);
    expect(screen.getByText(/上游返回 reasoning_content/)).toBeInTheDocument();
    expect(
      screen.getAllByText(/默认工具请求被拒绝或未返回有效工具调用/),
    ).toHaveLength(2);
    expect(screen.getByText(/原生推理历史结构不被接受/)).toBeInTheDocument();
    expect(screen.getByText(/本次上游已报告 1,500 tokens/)).toBeInTheDocument();
    expect(screen.getByText(/估算费用约 US\$0\.0123/)).toBeInTheDocument();
    expect(
      screen.getByText(/数字用量会随本次 Responses\/Chat 双协议脱敏证据保存/),
    ).toBeInTheDocument();
    expect(screen.getByText(/2 个成功响应未返回 usage/)).toBeInTheDocument();
    expect(
      screen.getByText(
        "运行时适配：Responses 请求保持原协议；续轮推理按 reasoning_text content 重建。",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "运行时适配：Codex Responses 转换为 Chat Completions；推理从 reasoning_content 读取并投影回 Codex。",
      ),
    ).toBeInTheDocument();

    const chatBranch = screen
      .getByText("Chat Completions")
      .closest("section") as HTMLElement;
    fireEvent.click(
      within(chatBranch).getByRole("button", {
        name: "查看字段映射与生效方式",
      }),
    );
    expect(
      within(chatBranch).getByText("Codex input[] → Chat messages[]"),
    ).toBeInTheDocument();
    expect(
      within(chatBranch).getByText(
        "function_call_output → role=tool + tool_call_id",
      ),
    ).toBeInTheDocument();
    expect(
      within(chatBranch).getByText(
        "choices[].delta.reasoning_content → response.reasoning_text.delta",
      ),
    ).toBeInTheDocument();
    expect(
      within(chatBranch).getByText(
        "保存并启用 Provider 后，每次运行请求都会自动应用；仅运行探测不会改写当前配置。",
      ),
    ).toBeInTheDocument();

    const responsesBranch = screen
      .getByText("Responses")
      .closest("section") as HTMLElement;
    fireEvent.click(
      within(responsesBranch).getByRole("button", {
        name: "查看字段映射与生效方式",
      }),
    );
    expect(
      within(responsesBranch).getByText(
        "reasoning.content / 可读 summary → content[type=reasoning_text]；summary=[]",
      ),
    ).toBeInTheDocument();
    expect(
      within(responsesBranch).getByText(
        "工具调用与工具结果保持 Responses 原结构",
      ),
    ).toBeInTheDocument();
  });

  it("warns before completion that deep probing sends billable model requests", () => {
    render(
      <CodexProtocolProbeProgressDialog
        open
        running
        expectedModels={["qwen3.8"]}
        events={[]}
        outcome={null}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(
      screen.getByText(
        /深度探测会向每个模型的 Responses 和 Chat 端点发送多次真实请求，会消耗 Token 并可能产生费用/,
      ),
    ).toBeInTheDocument();
  });

  it("shows a real repair attempt as pending and does not promote a failed retry", () => {
    const events: CodexProtocolProbeProgressEvent[] = [
      {
        kind: "compatibility_retry",
        model: "glm-test",
        transport: "open_ai_responses",
        stage: "continuation",
        rule: "omit_reasoning",
      },
    ];
    const props = {
      open: true,
      running: true,
      expectedModels: ["glm-test"],
      events,
      outcome: null,
      error: "",
      onOpenChange: vi.fn(),
    };
    const { rerender } = render(
      <CodexProtocolProbeProgressDialog {...props} />,
    );
    expect(screen.getByText("正在自动适配并重试")).toBeInTheDocument();
    expect(
      screen.getByText(/仅移除推理项，保留工具调用和工具结果/),
    ).toBeInTheDocument();
    expect(screen.queryByText("适配后通过")).not.toBeInTheDocument();
    rerender(
      <CodexProtocolProbeProgressDialog
        {...props}
        running={false}
        events={[
          ...events,
          {
            kind: "branch_finished",
            model: "glm-test",
            transport: "open_ai_responses",
            readiness: "partial",
          },
        ]}
      />,
    );
    expect(screen.getByText("适配仍未通过")).toBeInTheDocument();
    expect(screen.queryByText("正在自动适配并重试")).not.toBeInTheDocument();
  });

  it("aggregates reported token usage and mappings across provider outcomes", () => {
    const makeOutcome = (
      providerId: string,
      model: string,
      totalTokens: number,
      estimatedCostUsd: string,
    ) =>
      ({
        provider: {
          id: providerId,
          name: providerId,
          settingsConfig: {},
        },
        adaptationPreview: {
          persistence: "single",
          status: "ready",
          effectiveTransport: "open_ai_responses",
          models: [],
        },
        receiptIds: [`receipt-${providerId}`],
        protocolApplied: true,
        probeUsage: {
          inputTokens: totalTokens - 10,
          outputTokens: 10,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          totalTokens,
          reportedResponses: 1,
          successfulResponses: 1,
          estimatedCostUsd,
          pricedModels: [model],
          unpricedModels: [],
        },
        observations: [],
        records: [
          {
            probeVersion: 6,
            target: {
              provider_id: providerId,
              route_id: null,
              public_model: model,
              upstream_model: model,
              transport: "open_ai_responses",
              endpoint_fingerprint: `endpoint-${providerId}`,
              authentication_kind: "bearer",
              credential_fingerprint: `credential-${providerId}`,
              request_policy_fingerprint: `policy-${providerId}`,
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
      }) as CodexProviderProtocolPreflightOutcome;

    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["qwen3.8", "glm-5.3"]}
        expectedTargets={[
          {
            providerId: "qwen-provider",
            providerName: "qwen-provider",
            model: "qwen3.8",
          },
          {
            providerId: "glm-provider",
            providerName: "glm-provider",
            model: "glm-5.3",
          },
        ]}
        events={[]}
        outcome={null}
        outcomes={[
          makeOutcome("qwen-provider", "qwen3.8", 100, "0.001"),
          makeOutcome("glm-provider", "glm-5.3", 200, "0.002"),
        ]}
        error=""
        onOpenChange={vi.fn()}
      />,
    );

    expect(screen.getByText(/本次上游已报告 300 tokens/)).toBeInTheDocument();
    expect(screen.getByText(/估算费用约 US\$0\.003/)).toBeInTheDocument();
    expect(screen.getByText("qwen3.8")).toBeInTheDocument();
    expect(screen.getByText("glm-5.3")).toBeInTheDocument();
    expect(
      screen.getAllByText(
        "运行时适配：Responses 请求保持原协议；上游原生推理项按原结构回放。",
      ),
    ).toHaveLength(2);
  });

  it("marks opaque reasoning as unavailable for Codex presentation", () => {
    const outcome = {
      provider: {
        id: "provider",
        name: "Opaque gateway",
        settingsConfig: {},
      },
      adaptationPreview: {
        persistence: "single",
        status: "ready",
        effectiveTransport: "open_ai_chat",
        models: [],
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

  it("never reports 0/0/0 as a completed probe when expected models have no result", () => {
    render(
      <CodexProtocolProbeProgressDialog
        open
        running={false}
        expectedModels={["qwen3.8"]}
        events={[
          {
            kind: "batch_finished",
            total: 0,
            verified: 0,
            partial: 0,
            failed: 0,
          },
        ]}
        outcome={null}
        error="后端没有返回已启用模型 qwen3.8 的探测结果"
        onOpenChange={vi.fn()}
      />,
    );

    expect(
      screen.queryByText("已完成 0 个模型：Verified 0，Partial 0，Failed 0。"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("探测未完成：1 个模型没有结果。"),
    ).toBeInTheDocument();
    expect(screen.getAllByText("未返回探测结果")).toHaveLength(2);
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
