import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";

import { proxyApi } from "@/lib/api/proxy";
import type { CodexHistoryContentRepairOutcome } from "@/types/proxy";
import { CodexHistoryRepairPanel } from "./CodexHistoryRepairPanel";
import { CodexHistoryContentRepairPanel } from "./CodexHistoryContentRepairPanel";

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    previewCodexHistoryContentRepair: vi.fn(),
    applyCodexHistoryContentRepair: vi.fn(),
    repairCodexHistoryVisibility: vi.fn(),
    listCodexHistorySessions: vi.fn(),
    unlockCodexModelPicker: vi.fn(),
  },
}));

function outcome(
  overrides: Partial<CodexHistoryContentRepairOutcome> = {},
): CodexHistoryContentRepairOutcome {
  return {
    dryRun: true,
    codexHome: "C:/synthetic/.codex",
    previewToken: "synthetic-plan",
    planPath: "C:/synthetic/plans/plan.json",
    backupDir: null,
    scannedFiles: 12,
    affectedFiles: 2,
    compressedFiles: 1,
    repairedParts: 4,
    nativeConfig: true,
    requiresCodexExit: true,
    stage: "preview_ready",
    message: "合成预览结果",
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const preview = vi.mocked(proxyApi.previewCodexHistoryContentRepair);
const apply = vi.mocked(proxyApi.applyCodexHistoryContentRepair);

describe("Codex 旧对话续聊修复", () => {
  beforeEach(() => {
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    preview.mockReset().mockResolvedValue(outcome());
    apply.mockReset().mockResolvedValue(
      outcome({
        dryRun: false,
        previewToken: null,
        backupDir: "C:/synthetic/backups/complete",
        stage: "complete",
        message: "合成恢复完成",
      }),
    );
  });

  it("保留原默认界面和会话浏览，新增按钮独立打开续聊修复弹窗", async () => {
    const onClose = vi.fn();
    render(
      <CodexHistoryRepairPanel
        initialProjectPath="E:/other-project"
        onClose={onClose}
      />,
    );
    expect(
      screen.getByRole("button", { name: "确认修复" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(preview).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "旧对话续聊修复" }),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "预览续聊修复" }),
    ).toBeInTheDocument();
    expect(proxyApi.repairCodexHistoryVisibility).not.toHaveBeenCalled();
    expect(preview).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "关闭续聊修复" }));
    await userEvent.click(screen.getByRole("button", { name: "会话浏览" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("保留显式 MultiRouter 安装向导，独立弹窗不自动扫描", async () => {
    render(<CodexHistoryRepairPanel showAutomationGuide />);
    expect(
      screen.getByText(
        "MultiRouter 已配置成功，修复前先完全退出 Codex / ChatGPT App",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "预览续聊修复" }),
    ).not.toBeInTheDocument();
    expect(preview).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "旧对话续聊修复" }),
    );
    expect(
      screen.getByRole("button", { name: "预览续聊修复" }),
    ).toBeInTheDocument();
    expect(preview).not.toHaveBeenCalled();
  });

  it("只以 CodexHome 预览全目录，显式执行当前计划，不调用旧 API", async () => {
    render(<CodexHistoryRepairPanel initialProjectPath="E:/other-project" />);
    await userEvent.click(
      screen.getByRole("button", { name: "旧对话续聊修复" }),
    );
    await userEvent.type(
      within(screen.getByRole("dialog")).getByLabelText("Codex 目录"),
      " C:/synthetic/.codex ",
    );
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    expect(preview).toHaveBeenCalledTimes(1);
    expect(preview).toHaveBeenCalledWith("C:/synthetic/.codex");
    expect(screen.getByText("等待退出 Codex")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeEnabled();
    expect(apply).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "执行续聊修复" }));
    await screen.findByText("合成恢复完成");
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith("synthetic-plan");
    expect(
      screen.getByText("C:/synthetic/backups/complete"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeDisabled();
    expect(proxyApi.repairCodexHistoryVisibility).not.toHaveBeenCalled();
    expect(proxyApi.unlockCodexModelPicker).not.toHaveBeenCalled();
  });

  it("零命中是无需修复，不能写入", async () => {
    preview.mockResolvedValue(
      outcome({ affectedFiles: 0, compressedFiles: 0, repairedParts: 0 }),
    );
    render(<CodexHistoryContentRepairPanel />);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    expect(preview).toHaveBeenCalledWith(null);
    expect(
      await screen.findByText("未发现已识别的明文误标，无需修复。"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeDisabled();
    expect(apply).not.toHaveBeenCalled();
  });

  it("路由配置下可以预览并提示切回官方，执行仍交后端重新核验配置", async () => {
    preview.mockResolvedValue(outcome({ nativeConfig: false }));
    render(<CodexHistoryContentRepairPanel />);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    expect(
      await screen.findByText(/请先切回官方原生 OpenAI 配置/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeEnabled();
    expect(apply).not.toHaveBeenCalled();
  });

  it("改目录立即清除计划，过期异步预览不能污染新目录", async () => {
    const oldPreview = deferred<CodexHistoryContentRepairOutcome>();
    preview.mockReturnValueOnce(oldPreview.promise);
    render(<CodexHistoryContentRepairPanel />);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    fireEvent.change(screen.getByLabelText("Codex 目录"), {
      target: { value: "C:/new-home" },
    });
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    await screen.findByText("合成预览结果");
    await act(async () =>
      oldPreview.resolve(
        outcome({ previewToken: "stale", message: "旧目录结果" }),
      ),
    );
    expect(screen.queryByText("旧目录结果")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "执行续聊修复" }));
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith("synthetic-plan");
  });

  it("重新预览期间旧 token 失效，预览失败后也不能执行旧计划", async () => {
    render(<CodexHistoryContentRepairPanel />);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    await screen.findByText("合成预览结果");
    const failedPreview = deferred<CodexHistoryContentRepairOutcome>();
    preview.mockReturnValueOnce(failedPreview.promise);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeDisabled();
    await act(async () =>
      failedPreview.reject(new Error("synthetic preview failure <b>")),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "synthetic preview failure <b>",
    );
    expect(
      screen.queryByText("C:/synthetic/plans/plan.json"),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeDisabled();
  });

  it("写入拒绝后原样显示错误并清除计划，需重新预览", async () => {
    apply.mockRejectedValue(
      new Error("Codex still running: synthetic-process"),
    );
    render(<CodexHistoryContentRepairPanel />);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    await screen.findByText("合成预览结果");
    await userEvent.click(screen.getByRole("button", { name: "执行续聊修复" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Codex still running: synthetic-process",
    );
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeDisabled();
    expect(
      screen.queryByText("C:/synthetic/plans/plan.json"),
    ).not.toBeInTheDocument();
  });

  it("写入中禁用重复执行和目录变更，只显示真实待处理阶段", async () => {
    const pendingApply = deferred<CodexHistoryContentRepairOutcome>();
    apply.mockReturnValueOnce(pendingApply.promise);
    render(<CodexHistoryContentRepairPanel />);
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    await screen.findByText("合成预览结果");
    const button = screen.getByRole("button", { name: "执行续聊修复" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(apply).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Codex 目录")).toBeDisabled();
    expect(
      screen.getByText("正在检查退出状态、备份、写入并读回校验…"),
    ).toBeInTheDocument();
    await act(async () =>
      pendingApply.resolve(outcome({ dryRun: false, stage: "complete" })),
    );
    await waitFor(() => expect(button).toBeDisabled());
  });

  it("执行中不能关闭弹窗，完成后保留回执并允许关闭", async () => {
    const pendingApply = deferred<CodexHistoryContentRepairOutcome>();
    apply.mockReturnValueOnce(pendingApply.promise);
    const onClose = vi.fn();
    render(<CodexHistoryRepairPanel onClose={onClose} />);
    await userEvent.click(
      screen.getByRole("button", { name: "旧对话续聊修复" }),
    );
    await userEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    await screen.findByText("合成预览结果");
    await userEvent.click(screen.getByRole("button", { name: "执行续聊修复" }));
    const close = screen.getByRole("button", { name: "关闭续聊修复" });
    expect(close).toBeDisabled();
    await userEvent.click(close);
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    await act(async () =>
      pendingApply.resolve(outcome({ dryRun: false, message: "回执可见" })),
    );
    expect(await screen.findByText("回执可见")).toBeInTheDocument();
    expect(close).toBeEnabled();
    await userEvent.click(close);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("目录改变后成功的旧计划失效，计划路径支持复制", async () => {
    const clipboard = { writeText: vi.fn().mockResolvedValue(undefined) };
    Object.defineProperty(navigator, "clipboard", {
      value: clipboard,
      configurable: true,
    });
    render(<CodexHistoryContentRepairPanel />);
    fireEvent.click(screen.getByRole("button", { name: "预览续聊修复" }));
    await screen.findByText("合成预览结果");
    fireEvent.click(screen.getByRole("button", { name: "复制预览计划路径" }));
    expect(clipboard.writeText).toHaveBeenCalledWith(
      "C:/synthetic/plans/plan.json",
    );
    fireEvent.change(screen.getByLabelText("Codex 目录"), {
      target: { value: "C:/another" },
    });
    expect(screen.getByRole("button", { name: "执行续聊修复" })).toBeDisabled();
    expect(
      screen.queryByText("C:/synthetic/plans/plan.json"),
    ).not.toBeInTheDocument();
  });
});
