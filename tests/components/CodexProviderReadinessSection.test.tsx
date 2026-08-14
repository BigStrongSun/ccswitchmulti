import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CodexProviderReadinessSection } from "@/components/providers/forms/CodexProviderReadinessSection";

describe("CodexProviderReadinessSection", () => {
  it("keeps model synchronization and connection validation in the main flow", () => {
    const onSyncModels = vi.fn();
    const onValidateConnection = vi.fn();

    render(
      <CodexProviderReadinessSection
        models={[]}
        apiFormat="openai_chat"
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        onSyncModels={onSyncModels}
        onValidateConnection={onValidateConnection}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "模型与兼容性" }),
    ).toBeInTheDocument();
    expect(screen.getByText("就绪状态")).toBeInTheDocument();
    expect(screen.getByText("需要同步模型")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "同步模型" }));
    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));

    expect(onSyncModels).toHaveBeenCalledTimes(1);
    expect(onValidateConnection).toHaveBeenCalledTimes(1);
  });

  it("shows maintained preset ownership and a routable state without asking for protocol selection", () => {
    render(
      <CodexProviderReadinessSection
        models={[{ model: "deepseek-v4-flash" }, { model: "deepseek-v4-pro" }]}
        defaultModel="deepseek-v4-flash"
        apiFormat="openai_responses"
        isMaintainedPreset
        isSyncingModels={false}
        isValidatingConnection={false}
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByText("由 CCSwitchMulti 维护")).toBeInTheDocument();
    expect(screen.getByText("可加入 MultiRouter")).toBeInTheDocument();
    expect(screen.getByText("deepseek-v4-flash")).toBeInTheDocument();
    expect(screen.queryByText("请选择上游协议")).not.toBeInTheDocument();
  });

  it("explains automatic protocol detection for custom providers", () => {
    render(
      <CodexProviderReadinessSection
        models={[{ model: "private-model" }]}
        apiFormat="openai_chat"
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(
      screen.getByText(/验证连接时会自动检测 Chat 与 Responses/),
    ).toBeInTheDocument();
    expect(screen.getByText("建议先验证连接")).toBeInTheDocument();
  });

  it("uses accessible live regions for validation results", () => {
    const { rerender } = render(
      <CodexProviderReadinessSection
        models={[{ model: "private-model" }]}
        apiFormat="openai_chat"
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        validationSummary="Responses 和 Chat 均不可用"
        validationTone="error"
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Responses 和 Chat 均不可用",
    );

    rerender(
      <CodexProviderReadinessSection
        models={[{ model: "private-model" }]}
        apiFormat="openai_chat"
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        validationSummary="Chat 验证通过"
        validationTone="success"
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("Chat 验证通过");
  });
});
