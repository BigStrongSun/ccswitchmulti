import { render, screen } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";
import { Form } from "@/components/ui/form";

function FormShell({ children }: PropsWithChildren) {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
}

describe("OpenClaw Token Exchange fields", () => {
  it("keeps the TE safety shell when the provider type is TE but the descriptor is malformed", () => {
    render(
      <FormShell>
        <OpenClawFormFields
          baseUrl="https://attacker.example/v1"
          onBaseUrlChange={vi.fn()}
          apiKey="accidental-secret"
          onApiKeyChange={vi.fn()}
          shouldShowApiKeyLink={false}
          websiteUrl=""
          api="openai-completions"
          onApiChange={vi.fn()}
          models={[{ id: "ordinary-model", name: "Ordinary Model" }]}
          onModelsChange={vi.fn()}
          userAgent={false}
          onUserAgentChange={vi.fn()}
          teProvider={null}
          onTeProviderChange={vi.fn()}
          {...({ isTokenExchangeProvider: true } as Record<string, unknown>)}
        />
      </FormShell>,
    );

    expect(screen.getByTestId("te-provider-fields")).toBeInTheDocument();
    expect(screen.getByTestId("te-provider-errors")).toBeInTheDocument();
    expect(screen.queryByLabelText("API 端点")).not.toBeInTheDocument();
    expect(screen.queryByText("API 协议")).not.toBeInTheDocument();
    expect(screen.queryByText("模型列表")).not.toBeInTheDocument();
  });

  it("hides generic endpoint, API key, protocol and full model controls", () => {
    render(
      <FormShell>
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
        />
      </FormShell>,
    );

    expect(screen.getByTestId("te-provider-fields")).toBeInTheDocument();
    expect(screen.queryByLabelText("API 端点")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("发送 User-Agent")).not.toBeInTheDocument();
    expect(screen.queryByText("模型列表")).not.toBeInTheDocument();
    expect(screen.queryByText("API 协议")).not.toBeInTheDocument();
  });
});
