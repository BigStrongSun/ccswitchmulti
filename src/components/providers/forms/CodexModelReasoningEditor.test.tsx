import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { CodexModelReasoningCapability } from "@/types";

import { CodexModelReasoningEditor } from "./CodexModelReasoningEditor";

const qwenCapability: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "graded",
  supportedEfforts: ["low", "medium", "high"],
  defaultEffort: "medium",
  disableAllowed: false,
  upstream: {
    format: "string",
    parameter: "reasoning_effort",
    effortMap: { low: "low", medium: "medium", high: "high" },
  },
  source: "user",
};

describe("CodexModelReasoningEditor", () => {
  it("explains every manual configuration group", () => {
    render(
      <CodexModelReasoningEditor
        model="qwen3.8"
        capability={qwenCapability}
        readOnly={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("Provider 原生能力")).toBeInTheDocument();
    expect(screen.getByText("Provider 默认档位")).toBeInTheDocument();
    expect(screen.getByText("上游传参")).toBeInTheDocument();
    expect(screen.getByText("Codex → Provider 映射")).toBeInTheDocument();
    expect(screen.getByText(/同名档位会自动生成恒等映射/)).toBeInTheDocument();
    expect(screen.getByText(/例如：reasoning_effort/)).toBeInTheDocument();
  });

  it("shows mapping rows only for this model's Provider-native efforts", () => {
    render(
      <CodexModelReasoningEditor
        model="qwen3.8"
        capability={qwenCapability}
        readOnly={false}
        onChange={vi.fn()}
      />,
    );

    const mapping = screen.getByRole("group", {
      name: "qwen3.8 Codex 到 Provider 映射",
    });
    expect(within(mapping).getByLabelText("low 映射目标")).toHaveValue("low");
    expect(within(mapping).getByLabelText("medium 映射目标")).toHaveValue(
      "medium",
    );
    expect(within(mapping).getByLabelText("high 映射目标")).toHaveValue("high");
    expect(within(mapping).queryByLabelText("xhigh 映射目标")).toBeNull();
    expect(screen.queryByLabelText("Provider 原生档位 xhigh")).toBeNull();
  });

  it("fills the identity mapping when a native effort is selected", () => {
    const onChange = vi.fn();
    render(
      <CodexModelReasoningEditor
        model="qwen3.8"
        capability={{
          ...qwenCapability,
          supportedEfforts: ["low", "medium"],
          upstream: {
            ...qwenCapability.upstream,
            effortMap: { low: "low", medium: "medium" },
          },
        }}
        readOnly={false}
        onChange={onChange}
      />,
    );

    fireEvent.change(screen.getByLabelText("添加 Provider 原生档位"), {
      target: { value: "high" },
    });
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        supportedEfforts: ["low", "medium", "high"],
        upstream: expect.objectContaining({
          effortMap: expect.objectContaining({ high: "high" }),
        }),
      }),
    );
  });
});
