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
        "codexConfigConsistency.refreshCodex": "刷新 Codex 状态",
        "codexConfigConsistency.statusTitle": "Codex 状态与修复",
        "codexConfigConsistency.statusDescription":
          "配置、分页历史与界面兼容性分别检查。",
        "codexConfigConsistency.configStatus": "配置与运行态",
        "codexConfigConsistency.paginatedHistoryStatus": "分页历史",
        "codexConfigConsistency.rendererCompatibilityStatus": "历史查询兼容层",
        "codexConfigConsistency.statusReady": "正常",
        "codexConfigConsistency.statusWarning": "需要处理",
        "codexConfigConsistency.statusPending": "重启后验证",
        "codexConfigConsistency.noPaginatedHistoryIssue":
          "未发现可安全修复的重复序号。",
        "codexConfigConsistency.rendererIndependentHint":
          "此项失败不会撤销配置与分页历史修复。",
        "codexConfigConsistency.refreshConfirmTitle": "确认刷新 Codex 状态",
        "codexConfigConsistency.refreshConfirmDescription":
          "将关闭所有 Codex 窗口并中断正在运行的任务。",
        "codexConfigConsistency.desktopProcesses": "桌面主进程：",
        "codexConfigConsistency.appServerProcesses": "app-server：",
        "codexConfigConsistency.paginatedHistoryRepair":
          "检测到可安全恢复的分页历史投影",
        "codexConfigConsistency.paginatedHistoryFiles": "历史文件",
        "codexConfigConsistency.duplicateOrdinals": "重复序号",
        "codexConfigConsistency.paginatedHistorySkipped":
          "其他异常历史保持原样：",
        "codexConfigConsistency.historyCompatibilityCheck":
          "会备份原始 JSONL、保留全部对话并恢复分页投影。",
        "codexConfigConsistency.confirmRefresh": "关闭、重写并重新打开",
        "codexConfigConsistency.cancelRefresh": "取消",
        "codexConfigConsistency.refreshingTitle": "正在刷新 Codex 状态",
        "codexConfigConsistency.refreshingDescription":
          "请保持 CCSM 运行，Codex 将自动重新打开。",
        "codexConfigConsistency.stageClosing": "关闭 Codex",
        "codexConfigConsistency.stageRepairingHistory": "恢复分页历史",
        "codexConfigConsistency.stageApplyingConfig": "应用 CCSM 配置",
        "codexConfigConsistency.stageLaunching": "重新打开 Codex",
        "codexConfigConsistency.stageVerifying": "验证新运行态",
        "codexConfigConsistency.failedStage": "失败阶段：",
        "codexConfigConsistency.refreshFailedTitle": "Codex 状态刷新失败",
        "codexConfigConsistency.refreshFailedDescription":
          "流程已停在失败阶段。",
        "codexConfigConsistency.refreshCompletedTitle": "Codex 状态已刷新",
        "codexConfigConsistency.refreshCompletedDescription": "刷新完成。",
        "codexConfigConsistency.refreshCompletedWithWarningsTitle":
          "Codex 核心状态已刷新",
        "codexConfigConsistency.refreshCompletedWithWarningsDescription":
          "配置和分页历史已完成，界面兼容层仍需处理。",
        "codexConfigConsistency.configApplied": "配置与运行态已生效",
        "codexConfigConsistency.paginatedHistoryReady": "分页历史已校验",
        "codexConfigConsistency.rendererCompatibilityWarning":
          "历史查询兼容层未就绪",
        "codexConfigConsistency.retryRendererCompatibility":
          "重试历史查询兼容层",
        "codexConfigConsistency.newTaskHint": "请新建任务。",
        "codexConfigConsistency.historyRepairCompleted": "已恢复分页历史文件",
        "codexConfigConsistency.finish": "完成",
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

  it("checks the exact Codex runtime before offering the destructive refresh", () => {
    const onInspectRefresh = vi.fn();
    const onConfirmRefresh = vi.fn();
    const runtimeReport: CodexConfigConsistencyReport = {
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
    };
    const { rerender } = render(
      <CodexConfigConsistencyDialog
        report={runtimeReport}
        pending={false}
        error={null}
        refresh={{ phase: "idle", preflight: null, progress: null }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={onInspectRefresh}
        onConfirmRefresh={onConfirmRefresh}
        onCancelRefresh={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "刷新 Codex 状态" }));
    expect(onInspectRefresh).toHaveBeenCalledOnce();

    rerender(
      <CodexConfigConsistencyDialog
        report={runtimeReport}
        pending={false}
        error={null}
        refresh={{
          phase: "confirm",
          progress: null,
          preflight: {
            supported: true,
            canRefresh: true,
            snapshotToken: "snapshot",
            desktopProcessCount: 1,
            appServerProcessCount: 1,
            processCount: 2,
            launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
            warning: "active_tasks_will_be_interrupted",
            paginatedHistory: {
              affectedRolloutCount: 1,
              duplicateOrdinalCount: 3,
              affectedBytes: 1_100_000_000,
              blockedRolloutCount: 1,
              blockedReason: "unsafe_rollout_ordinal_sequence",
            },
          },
        }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={onInspectRefresh}
        onConfirmRefresh={onConfirmRefresh}
        onCancelRefresh={vi.fn()}
      />,
    );

    const confirmation = screen.getByRole("dialog", {
      name: "确认刷新 Codex 状态",
    });
    expect(
      within(confirmation).getByText(/桌面主进程：.*1/),
    ).toBeInTheDocument();
    expect(
      within(confirmation).getByText(/app-server：.*1/),
    ).toBeInTheDocument();
    expect(
      within(confirmation).getByText(
        "会备份原始 JSONL、保留全部对话并恢复分页投影。",
      ),
    ).toBeInTheDocument();
    expect(
      within(confirmation).getByText("检测到可安全恢复的分页历史投影"),
    ).toBeInTheDocument();
    expect(within(confirmation).getByText(/历史文件.*1/)).toBeInTheDocument();
    expect(within(confirmation).getByText(/重复序号.*3/)).toBeInTheDocument();
    expect(
      within(confirmation).getByText(/其他异常历史保持原样：.*1/),
    ).toBeInTheDocument();
    fireEvent.click(
      within(confirmation).getByRole("button", {
        name: "关闭、重写并重新打开",
      }),
    );
    expect(onConfirmRefresh).toHaveBeenCalledOnce();
  });

  it("shows all five refresh stages in one modal instead of stacking another dialog", () => {
    render(
      <CodexConfigConsistencyDialog
        report={report}
        pending={false}
        error={null}
        refresh={{
          phase: "refreshing",
          preflight: {
            supported: true,
            canRefresh: true,
            snapshotToken: "snapshot",
            desktopProcessCount: 1,
            appServerProcessCount: 1,
            processCount: 2,
            launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
            warning: "active_tasks_will_be_interrupted",
            paginatedHistory: {
              affectedRolloutCount: 1,
              duplicateOrdinalCount: 3,
              affectedBytes: 1_100_000_000,
              blockedRolloutCount: 0,
              blockedReason: null,
            },
          },
          progress: { stage: "applying_config" },
        }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={vi.fn()}
        onConfirmRefresh={vi.fn()}
        onCancelRefresh={vi.fn()}
      />,
    );

    const dialog = screen.getByRole("dialog", {
      name: "正在刷新 Codex 状态",
    });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(within(dialog).getByText("关闭 Codex")).toBeInTheDocument();
    expect(within(dialog).getByText("恢复分页历史")).toBeInTheDocument();
    expect(within(dialog).getByText("应用 CCSM 配置")).toBeInTheDocument();
    expect(within(dialog).getByText("重新打开 Codex")).toBeInTheDocument();
    expect(within(dialog).getByText("验证新运行态")).toBeInTheDocument();
  });

  it("keeps the exact failed stage visible for a retry", () => {
    render(
      <CodexConfigConsistencyDialog
        report={report}
        pending={false}
        error={null}
        refresh={{
          phase: "failed",
          preflight: null,
          progress: { stage: "launching" },
          error: "AUMID 启动失败",
        }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={vi.fn()}
        onConfirmRefresh={vi.fn()}
        onCancelRefresh={vi.fn()}
      />,
    );

    expect(screen.getByText("失败阶段： 重新打开 Codex")).toBeInTheDocument();
    expect(screen.getByText("AUMID 启动失败")).toBeInTheDocument();
  });

  it("reports the exact paginated history repair outcome after completion", () => {
    render(
      <CodexConfigConsistencyDialog
        report={report}
        pending={false}
        error={null}
        refresh={{
          phase: "completed",
          preflight: null,
          progress: { stage: "completed" },
          result: {
            outcome: "completed",
            configStatus: "ready",
            paginatedHistoryStatus: "ready",
            rendererCompatibilityStatus: "ready",
            rendererCompatibilityMessage: null,
            forceTerminated: false,
            closedProcessCount: 2,
            repairedHistoryRolloutCount: 3,
            repairedHistoryDuplicateCount: 5,
          },
        }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={vi.fn()}
        onConfirmRefresh={vi.fn()}
        onCancelRefresh={vi.fn()}
      />,
    );

    expect(screen.getByText(/已恢复分页历史文件.*3/)).toBeInTheDocument();
    expect(screen.getByText(/重复序号.*5/)).toBeInTheDocument();
  });

  it("keeps config and paginated history successful when renderer compatibility warns", () => {
    const onRetryRendererCompatibility = vi.fn();
    render(
      <CodexConfigConsistencyDialog
        report={null}
        pending={false}
        error={null}
        refresh={{
          phase: "completed",
          preflight: null,
          progress: { stage: "completed" },
          result: {
            outcome: "completed_with_warnings",
            configStatus: "ready",
            paginatedHistoryStatus: "ready",
            rendererCompatibilityStatus: "warning",
            rendererCompatibilityMessage:
              "renderer request client was not found",
            forceTerminated: false,
            closedProcessCount: 2,
            repairedHistoryRolloutCount: 1,
            repairedHistoryDuplicateCount: 4,
          },
        }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={vi.fn()}
        onConfirmRefresh={vi.fn()}
        onCancelRefresh={vi.fn()}
        onRetryRendererCompatibility={onRetryRendererCompatibility}
      />,
    );

    expect(
      screen.getByRole("dialog", { name: "Codex 核心状态已刷新" }),
    ).toBeInTheDocument();
    expect(screen.getByText("配置与运行态已生效")).toBeInTheDocument();
    expect(screen.getByText("分页历史已校验")).toBeInTheDocument();
    expect(screen.getByText("历史查询兼容层未就绪")).toBeInTheDocument();
    expect(
      screen.getByText("renderer request client was not found"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Codex 状态刷新失败")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "重试历史查询兼容层" }));
    expect(onRetryRendererCompatibility).toHaveBeenCalledOnce();
  });

  it("opens a manual status overview without requiring an inconsistency report", () => {
    const onInspectRefresh = vi.fn();
    render(
      <CodexConfigConsistencyDialog
        report={null}
        pending={false}
        error={null}
        refresh={{
          phase: "status",
          progress: null,
          preflight: {
            supported: true,
            canRefresh: true,
            snapshotToken: "snapshot",
            desktopProcessCount: 1,
            appServerProcessCount: 1,
            processCount: 2,
            launchTarget: "OpenAI.Codex_2p2nqsd0c76g0!App",
            warning: null,
            paginatedHistory: {
              affectedRolloutCount: 0,
              duplicateOrdinalCount: 0,
              affectedBytes: 0,
              blockedRolloutCount: 0,
              blockedReason: null,
            },
          },
        }}
        onApply={vi.fn()}
        onKeep={vi.fn()}
        onLater={vi.fn()}
        onRetry={vi.fn()}
        onInspectRefresh={onInspectRefresh}
        onConfirmRefresh={vi.fn()}
        onCancelRefresh={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("dialog", { name: "Codex 状态与修复" }),
    ).toBeInTheDocument();
    expect(screen.getByText("配置与运行态")).toBeInTheDocument();
    expect(screen.getByText("分页历史")).toBeInTheDocument();
    expect(screen.getByText("历史查询兼容层")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "刷新 Codex 状态" }));
    expect(onInspectRefresh).toHaveBeenCalledOnce();
  });
});
