import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { CodexConfigConsistencyReport } from "@/lib/api/codexConfigConsistency";
import { CodexConfigConsistencyDialog } from "./CodexConfigConsistencyDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        "codexConfigConsistency.title": "Codex 配置与 CCSM 不一致",
        "codexConfigConsistency.description":
          "检测到 Codex config.toml 在 CCSM 之外发生了修改。请选择如何处理。",
        "codexConfigConsistency.provider": "当前 Provider：",
        "codexConfigConsistency.changedKeys": "变更的配置项：",
        "codexConfigConsistency.noSecrets": "仅显示配置键路径，不显示配置值。",
        "codexConfigConsistency.retry": "重试应用",
        "codexConfigConsistency.later": "稍后处理",
        "codexConfigConsistency.keepCodex": "保留 Codex 修改",
        "codexConfigConsistency.applyCcsm": "应用 CCSM 配置",
        "common.unknown": "未知",
      })[key] ?? key,
  }),
}));

const report: CodexConfigConsistencyReport = {
  state: "external_drift",
  providerId: "router",
  expectedFingerprint: "expected",
  actualFingerprint: "actual",
  changedKeys: ["model_reasoning_effort", "features.web_search"],
  reason: "live_config_changed",
};

describe("CodexConfigConsistencyDialog", () => {
  it("shows changed key paths without values and exposes all decisions", () => {
    const onApply = vi.fn();
    const onKeep = vi.fn();
    const onLater = vi.fn();
    render(
      <CodexConfigConsistencyDialog
        report={report}
        pending={false}
        error={null}
        onApply={onApply}
        onKeep={onKeep}
        onLater={onLater}
        onRetry={vi.fn()}
      />,
    );

    const dialog = screen.getByRole("dialog", {
      name: "Codex 配置与 CCSM 不一致",
    });
    expect(
      within(dialog).getByText("model_reasoning_effort"),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("features.web_search")).toBeInTheDocument();
    expect(within(dialog).queryByText("expected")).toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "应用 CCSM 配置" }),
    );
    expect(onApply).toHaveBeenCalledOnce();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "保留 Codex 修改" }),
    );
    expect(onKeep).toHaveBeenCalledOnce();
    fireEvent.click(within(dialog).getByRole("button", { name: "稍后处理" }));
    expect(onLater).toHaveBeenCalledOnce();
  });

  it("keeps the dialog actionable when apply fails", () => {
    const onRetry = vi.fn();
    render(
      <CodexConfigConsistencyDialog
        report={report}
        pending={false}
        error="配置在确认后再次发生变化"
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={onRetry}
      />,
    );

    expect(screen.getByText("配置在确认后再次发生变化")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试应用" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
