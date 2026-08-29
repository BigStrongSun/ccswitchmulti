import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { CodexProviderSetPreview } from "@/lib/api/protocol-compatibility";
import { CodexProviderSetPreviewDialog } from "./CodexProviderSetPreviewDialog";

const splitPreview: CodexProviderSetPreview = {
  digest: "split-digest",
  sourceProviderId: "provider-source",
  responsesModels: ["qwen3.8", "glm-5"],
  chatModels: ["deepseek-v4"],
  plan: {
    kind: "split",
    responses_provider_id: "provider-source--ccsm-responses",
    chat_provider_id: "provider-source--ccsm-chat",
  },
};

describe("CodexProviderSetPreviewDialog", () => {
  it("shows the backend split groups without asking the user to choose protocols", () => {
    const onConfirmSplit = vi.fn();
    const onBack = vi.fn();

    render(
      <CodexProviderSetPreviewDialog
        open
        preview={splitPreview}
        pending={false}
        onBack={onBack}
        onConfirmSplit={onConfirmSplit}
        onRetry={vi.fn()}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "按协议自动拆分" });
    expect(within(dialog).getByText("qwen3.8")).toBeInTheDocument();
    expect(within(dialog).getByText("glm-5")).toBeInTheDocument();
    expect(within(dialog).getByText("deepseek-v4")).toBeInTheDocument();
    expect(within(dialog).getByText("Responses 模型")).toBeInTheDocument();
    expect(
      within(dialog).getByText("Chat Completions 模型"),
    ).toBeInTheDocument();
    expect(within(dialog).queryByRole("radio")).toBeNull();
    expect(within(dialog).queryByRole("combobox")).toBeNull();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "确认按协议拆分" }),
    );
    expect(onConfirmSplit).toHaveBeenCalledOnce();
    expect(onBack).not.toHaveBeenCalled();
  });

  it("blocks saving when any model lacks a verified unique selection", () => {
    const onRetry = vi.fn();
    const blocked: CodexProviderSetPreview = {
      digest: "blocked-digest",
      sourceProviderId: "provider-source",
      responsesModels: [],
      chatModels: [],
      plan: {
        kind: "blocked",
        models: [
          {
            model: "qwen3.8",
            upstreamModel: "Qwen/Qwen3.8",
            reason: "probe_not_verified",
            stage: "continuation",
            failureKind: "http_status",
            statusCode: 422,
          },
        ],
      },
    };

    render(
      <CodexProviderSetPreviewDialog
        open
        preview={blocked}
        pending={false}
        onBack={vi.fn()}
        onConfirmSplit={vi.fn()}
        onRetry={onRetry}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "暂时无法保存" });
    expect(within(dialog).getByText("qwen3.8")).toBeInTheDocument();
    expect(within(dialog).getByText("Qwen/Qwen3.8")).toBeInTheDocument();
    expect(
      within(dialog).getByText("探测结果尚未通过完整验证"),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("失败阶段：工具续轮")).toBeInTheDocument();
    expect(
      within(dialog).getByText("失败类型：HTTP 状态（422）"),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "确认按协议拆分" }),
    ).toBeNull();

    fireEvent.click(within(dialog).getByRole("button", { name: "重新探测" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
