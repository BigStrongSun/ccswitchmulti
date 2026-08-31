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
        "codexConfigConsistency.runtimeTitle": "Codex 仍在使用旧配置",
        "codexConfigConsistency.runtimeDescription":
          "磁盘配置已经正确，但运行中的 app-server 启动得更早。",
        "codexConfigConsistency.runtimeRestartHint":
          "请完全退出 Codex Desktop/app-server，重新打开后新建任务。",
        "codexConfigConsistency.recheck": "重新检测",
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
  runtimeActivation: {
    state: "current",
    appServerStartedAt: null,
    configModifiedAt: null,
    reason: null,
  },
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

  it("does not allow keeping a broken live projection while takeover remains enabled", () => {
    render(
      <CodexConfigConsistencyDialog
        report={{
          ...report,
          reason: "takeover_projection_drift",
          changedKeys: [
            "model_provider",
            "model_providers.codex_model_router_v2",
          ],
        }}
        pending={false}
        error={null}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
      />,
    );

    expect(screen.queryByText("保留 Codex 修改")).toBeNull();
    expect(screen.getByText("应用 CCSM 配置")).toBeInTheDocument();
  });

  it("shows restart guidance instead of pretending a disk-only repair is active", () => {
    const onRetry = vi.fn();
    render(
      <CodexConfigConsistencyDialog
        report={{
          ...report,
          state: "consistent",
          changedKeys: [],
          reason: null,
          runtimeActivation: {
            state: "restart_required",
            appServerStartedAt: "2026-08-31T21:48:34+08:00",
            configModifiedAt: "2026-08-31T22:38:53+08:00",
            reason: "app_server_started_before_managed_config",
          },
        }}
        pending={false}
        error={null}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={onRetry}
      />,
    );

    expect(
      screen.getByRole("dialog", { name: "Codex 仍在使用旧配置" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "请完全退出 Codex Desktop/app-server，重新打开后新建任务。",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("应用 CCSM 配置")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "重新检测" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
