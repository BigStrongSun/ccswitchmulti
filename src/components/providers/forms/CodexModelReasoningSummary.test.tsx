import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CodexModelReasoningSummary } from "./CodexModelReasoningSummary";

describe("CodexModelReasoningSummary", () => {
  it("keeps each model concise until the user opens its configuration", () => {
    const onToggle = vi.fn();

    render(
      <div>
        <CodexModelReasoningSummary
          model="qwen3.8"
          source="自动发现"
          selectableEfforts={["low", "medium", "high"]}
          defaultEffort="medium"
          ultraEnabled={false}
          expanded={false}
          onToggle={onToggle}
        />
        <CodexModelReasoningSummary
          model="qwen3.8-coder"
          source="用户声明"
          selectableEfforts={["low", "high"]}
          defaultEffort="high"
          ultraEnabled
          expanded={false}
          onToggle={onToggle}
        />
      </div>,
    );

    expect(screen.getByText("qwen3.8")).toBeInTheDocument();
    expect(screen.getByText("qwen3.8-coder")).toBeInTheDocument();
    expect(screen.getAllByText("配置推理能力")).toHaveLength(2);
    expect(
      screen.getByText("Codex 档位：low / medium / high"),
    ).toBeInTheDocument();
    expect(screen.getByText("Ultra：关闭")).toBeInTheDocument();
    expect(screen.getByText("Ultra：开启")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "配置 qwen3.8 的推理能力" }),
    );
    expect(onToggle).toHaveBeenCalledOnce();
  });
});
