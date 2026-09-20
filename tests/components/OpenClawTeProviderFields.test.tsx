import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";

describe("OpenClaw Token Exchange fields", () => {
  it("hides generic endpoint, API key, protocol and full model controls", () => {
    render(
      <OpenClawFormFields
        baseUrl="http://127.0.0.1:9814/v1"
        onBaseUrlChange={vi.fn()}
        apiKey="te-provider-placeholder-not-a-secret"
        onApiKeyChange={vi.fn()}
        shouldShowApiKeyLink={false}
        websiteUrl=""
        api="openai-completions"
        onApiChange={vi.fn()}
        models={[{ id: "qwen3.8", name: "Qwen 3.8" }]}
        onModelsChange={vi.fn()}
        userAgent={false}
        onUserAgentChange={vi.fn()}
        teProvider={{
          sidecarUrl: "http://127.0.0.1:9814",
          expectedPartnerAic: "partner-aic",
          protocolVersion: "te-provider.v1",
          bindingDelivery: "config-headers",
          models: [{ id: "qwen3.8", name: "Qwen 3.8" }],
        }}
        onTeProviderChange={vi.fn()}
      />,
    );

    expect(screen.getByTestId("te-provider-fields")).toBeInTheDocument();
    expect(screen.queryByLabelText("API 端点")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("发送 User-Agent")).not.toBeInTheDocument();
    expect(screen.queryByText("模型列表")).not.toBeInTheDocument();
    expect(screen.queryByText("API 协议")).not.toBeInTheDocument();
  });
});
