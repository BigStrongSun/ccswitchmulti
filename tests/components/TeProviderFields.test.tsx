import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { TeProviderFields } from "@/components/providers/forms/TeProviderFields";
import type { OpenClawTeProviderSettings } from "@/types";
import {
  TE_PROVIDER_PLACEHOLDER_API_KEY,
  buildOpenClawTeProviderConfig,
} from "@/utils/teProvider";

function baseSettings(): OpenClawTeProviderSettings {
  return {
    sidecarUrl: "http://127.0.0.1:9814",
    expectedPartnerAic: "1.2.156.3088.0001.00001.TTLIHU.LW9WCA.1.0N2P",
    protocolVersion: "te-provider.v1",
    bindingDelivery: "config-headers",
    providerTimeoutSeconds: 300,
    keepAliveIntervalSeconds: 30,
    models: [
      {
        id: "qwen3.8",
        name: "qwen3.8",
        inputModalities: ["text"],
        outputModalities: ["text"],
      },
    ],
  };
}

function Harness({ initial }: { initial: OpenClawTeProviderSettings }) {
  const [value, setValue] = useState(initial);
  return <TeProviderFields value={value} onChange={setValue} />;
}

describe("TeProviderFields", () => {
  it("edits non-secret binding fields and never offers a Proxy Key input", () => {
    render(<Harness initial={baseSettings()} />);

    const sidecar = screen.getByTestId("te-sidecar-url") as HTMLInputElement;
    fireEvent.change(sidecar, {
      target: { value: "http://127.0.0.1:19001" },
    });
    expect(sidecar.value).toBe("http://127.0.0.1:19001");

    // 运行时字段只读展示：不出现任何可输入 Proxy Key / Agent Credential 的控件。
    expect(screen.getAllByTestId("te-runtime-field").length).toBeGreaterThan(0);
    expect(screen.queryByLabelText(/proxy key/i)).toBeNull();
    expect(document.body.textContent).not.toContain("proxyKeyRef=");
  });

  it("collects model capability metadata that the injector projects into OpenClaw", () => {
    render(<Harness initial={baseSettings()} />);

    fireEvent.click(screen.getByTestId("te-model-input-image"));
    fireEvent.click(screen.getByTestId("te-model-supports-tools"));
    fireEvent.change(screen.getByTestId("te-model-context-window"), {
      target: { value: "131072" },
    });

    const projected = buildOpenClawTeProviderConfig({
      ...baseSettings(),
      models: [
        {
          id: "qwen3.8",
          name: "qwen3.8",
          inputModalities: ["text", "image"],
          outputModalities: ["text"],
          supportsTools: true,
          contextWindowTokens: 131072,
        },
      ],
    });
    expect(projected.baseUrl).toBe("http://127.0.0.1:9814/v1");
    expect(projected.apiKey).toBe(TE_PROVIDER_PLACEHOLDER_API_KEY);
    expect(projected.models?.[0]).toMatchObject({
      id: "qwen3.8",
      input: ["text", "image"],
      contextWindow: 131072,
      compat: { supportsTools: true },
    });
  });

  it("blocks invalid settings with deterministic messages instead of silently saving", () => {
    render(
      <Harness
        initial={{
          ...baseSettings(),
          sidecarUrl: "https://example.com",
          expectedPartnerAic: "",
          models: [],
        }}
      />,
    );

    const errors = screen.getByTestId("te-provider-errors");
    expect(errors.textContent).toContain("数值回环");
    expect(errors.textContent).toContain("Partner AIC");
    expect(errors.textContent).toContain("至少需要一个模型条目");
  });

  it("reports duplicated model ids before they reach the Agent config", () => {
    render(
      <Harness
        initial={{
          ...baseSettings(),
          models: [
            { id: "dup", name: "dup" },
            { id: "dup", name: "dup-2" },
          ],
        }}
      />,
    );

    expect(screen.getByTestId("te-provider-errors").textContent).toContain(
      "模型 ID 不能重复",
    );
  });
});
