import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CodexModelReasoningSummary } from "./CodexModelReasoningSummary";

describe("CodexModelReasoningSummary", () => {
  it("keeps each model concise until the user opens its configuration", () => {
    const onToggle = vi.fn();
    const onUltraChange = vi.fn();

    render(
      <div>
        <CodexModelReasoningSummary
          model="qwen3.8"
          source="自动发现"
          selectableEfforts={["low", "medium", "high"]}
          defaultEffort="medium"
          ultraEnabled={false}
          ultraEfforts={["low", "medium", "high"]}
          onUltraChange={onUltraChange}
          expanded={false}
          onToggle={onToggle}
        />
        <CodexModelReasoningSummary
          model="qwen3.8-coder"
          source="用户声明"
          selectableEfforts={["low", "high"]}
          defaultEffort="high"
          ultraEnabled
          ultraEffort="high"
          ultraEfforts={["low", "high"]}
          onUltraChange={onUltraChange}
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
    expect(
      screen.getByRole("checkbox", { name: "解锁 qwen3.8 的 Ultra 档" }),
    ).not.toBeChecked();
    expect(
      screen.getByRole("checkbox", {
        name: "解锁 qwen3.8-coder 的 Ultra 档",
      }),
    ).toBeChecked();

    fireEvent.click(
      screen.getByRole("button", { name: "配置 qwen3.8 的推理能力" }),
    );
    expect(onToggle).toHaveBeenCalledOnce();

    fireEvent.click(
      screen.getByRole("checkbox", { name: "解锁 qwen3.8 的 Ultra 档" }),
    );
    expect(onUltraChange).toHaveBeenNthCalledWith(1, {
      enabled: true,
      providerEffort: undefined,
    });
  });

  it("allows Ultra to be unlocked before Provider effort discovery and opens the required configuration", () => {
    const onToggle = vi.fn();
    const onUltraChange = vi.fn();

    render(
      <CodexModelReasoningSummary
        model="glm-4.5"
        source="自动发现或服务端默认"
        selectableEfforts={[]}
        ultraEnabled={false}
        ultraEfforts={[]}
        onUltraChange={onUltraChange}
        expanded={false}
        onToggle={onToggle}
      />,
    );

    const unlock = screen.getByRole("checkbox", {
      name: "解锁 glm-4.5 的 Ultra 档",
    });
    expect(unlock).toBeEnabled();

    fireEvent.click(unlock);

    expect(onUltraChange).toHaveBeenCalledWith({
      enabled: true,
      providerEffort: undefined,
    });
    expect(onToggle).toHaveBeenCalledOnce();
    expect(
      screen.getByText(
        "需要先确认该模型可接收的推理强度；已为你展开推理能力配置，完成后才能保存。",
      ),
    ).toBeInTheDocument();
  });
});
