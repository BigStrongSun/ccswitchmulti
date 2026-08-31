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

  it("keeps maintained metadata ownership visible without treating unverified credentials as ready", () => {
    const { rerender } = render(
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
    expect(screen.getByText("建议先验证连接")).toBeInTheDocument();
    expect(screen.queryByText("可加入 MultiRouter")).not.toBeInTheDocument();
    expect(screen.getByText("deepseek-v4-flash")).toBeInTheDocument();
    expect(screen.queryByText("请选择上游协议")).not.toBeInTheDocument();

    rerender(
      <CodexProviderReadinessSection
        models={[{ model: "deepseek-v4-flash" }, { model: "deepseek-v4-pro" }]}
        defaultModel="deepseek-v4-flash"
        apiFormat="openai_responses"
        isMaintainedPreset
        isSyncingModels={false}
        isValidatingConnection={false}
        validationSummary="当前凭据和端点验证通过"
        validationTone="success"
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByText("可加入 MultiRouter")).toBeInTheDocument();
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

  it("restores ready state from persisted protocol adaptation evidence", () => {
    render(
      <CodexProviderReadinessSection
        models={[{ model: "qwen3.8" }]}
        defaultModel="qwen3.8"
        apiFormat="openai_chat"
        adaptation={{
          persistence: "single",
          status: "ready",
          effectiveTransport: "open_ai_chat",
          testedAt: Math.floor(Date.now() / 1000) - 60,
          expiresAt: Math.floor(Date.now() / 1000) + 3600,
          models: [],
        }}
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByText("可加入 MultiRouter")).toBeInTheDocument();
    expect(screen.queryByText("建议先验证连接")).not.toBeInTheDocument();
  });

  it("does not let stale evidence hide a current validation failure", () => {
    const readyAdaptation = {
      persistence: "single" as const,
      status: "ready" as const,
      effectiveTransport: "open_ai_chat" as const,
      testedAt: Math.floor(Date.now() / 1000) - 60,
      expiresAt: Math.floor(Date.now() / 1000) + 3600,
      models: [],
    };
    const { rerender } = render(
      <CodexProviderReadinessSection
        models={[{ model: "qwen3.8" }]}
        apiFormat="openai_chat"
        adaptation={{ ...readyAdaptation, status: "stale" }}
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByText("建议先验证连接")).toBeInTheDocument();
    expect(screen.queryByText("可加入 MultiRouter")).not.toBeInTheDocument();

    rerender(
      <CodexProviderReadinessSection
        models={[{ model: "qwen3.8" }]}
        apiFormat="openai_chat"
        adaptation={readyAdaptation}
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        validationSummary="当前端点不可用"
        validationTone="error"
        onSyncModels={vi.fn()}
        onValidateConnection={vi.fn()}
      />,
    );

    expect(screen.getByText("连接验证失败")).toBeInTheDocument();
    expect(screen.queryByText("可加入 MultiRouter")).not.toBeInTheDocument();
  });

  it("distinguishes an all-disabled catalog from usable probe models", () => {
    const onValidateConnection = vi.fn();
    render(
      <CodexProviderReadinessSection
        models={[{ model: "disabled-model", enabled: false }]}
        defaultModel="disabled-model"
        apiFormat="openai_chat"
        isMaintainedPreset={false}
        isSyncingModels={false}
        isValidatingConnection={false}
        onSyncModels={vi.fn()}
        onValidateConnection={onValidateConnection}
      />,
    );

    expect(screen.getByText("0 个已启用，1 个已停用")).toBeInTheDocument();
    expect(screen.getByText("需要启用模型")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "验证连接" }));
    expect(onValidateConnection).toHaveBeenCalledOnce();
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
