import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { History, KeyRound, RotateCcw } from "lucide-react";
import { toast } from "sonner";
import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";

interface CodexAuthSettingsProps {
  settings: SettingsFormState;
  /** 返回 false（或 resolve 为 false）表示保存失败；其余返回值视为成功 */
  onChange: (
    updates: Partial<SettingsFormState>,
  ) => void | boolean | Promise<void | boolean>;
}

export function CodexAuthSettings({
  settings,
  onChange,
}: CodexAuthSettingsProps) {
  const { t } = useTranslation();
  const [showEnableConfirm, setShowEnableConfirm] = useState(false);
  const [showDisableConfirm, setShowDisableConfirm] = useState(false);
  const [hasUnifyBackup, setHasUnifyBackup] = useState(false);

  const handleUnifyHistoryChange = (checked: boolean) => {
    if (checked) {
      setShowEnableConfirm(true);
      return;
    }
    // 先探测有无迁移备份，决定关闭弹窗是否提供"恢复备份"勾选
    void settingsApi
      .hasCodexUnifyHistoryBackup()
      .catch(() => false)
      .then((hasBackup) => {
        setHasUnifyBackup(hasBackup);
        setShowDisableConfirm(true);
      });
  };

  const handleEnableConfirm = (migrateExisting: boolean) => {
    setShowEnableConfirm(false);
    void onChange({
      unifyCodexSessionHistory: true,
      unifyCodexMigrateExisting: migrateExisting,
    });
  };

  // 备份探测可能落后于正在后台进行的迁移（刚勾选迁入就立刻关闭时，
  // 备份尚未产出）。只要本轮勾选过"迁入既有会话"，就必须提供恢复入口；
  // 真正有没有账本交给后端 restore 的 skippedReason 判定。
  const showRestoreOption =
    hasUnifyBackup || (settings.unifyCodexMigrateExisting ?? false);

  const handleDisableConfirm = async (restoreBackup: boolean) => {
    setShowDisableConfirm(false);
    const saved = await onChange({
      unifyCodexSessionHistory: false,
      unifyCodexMigrateExisting: false,
    });
    // 关闭保存失败时绝不还原：否则开关仍开着（live 仍统一路由），
    // 已迁移会话却被翻回 openai 桶，历史被拆成两半。
    if (saved === false) return;
    // 不再以探测结果短路：还原命令会在迁移锁上排队，等到迁移落盘后
    // 拿到完整账本；确实无账本时由 skippedReason 提示。
    if (!restoreBackup) return;
    try {
      const result = await settingsApi.restoreCodexUnifiedHistory();
      if (result.skippedReason) {
        // unify_toggle_on：还原排队期间开关被重新开启，后端拒绝还原
        toast.info(
          result.skippedReason === "unify_toggle_on"
            ? t("settings.unifyCodexHistoryRestoreSkippedToggleOn")
            : t("settings.unifyCodexHistoryRestoreNothing"),
        );
        return;
      }
      toast.success(
        t("settings.unifyCodexHistoryRestoreCompleted", {
          files: result.restoredJsonlFiles,
          rows: result.restoredStateRows,
        }),
      );
    } catch (error) {
      console.error("Failed to restore codex unified history:", error);
      toast.error(t("settings.unifyCodexHistoryRestoreFailed"));
    }
  };

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <KeyRound className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">{t("settings.codexAuth")}</h3>
      </div>

      <ToggleRow
        icon={<KeyRound className="h-4 w-4 text-emerald-500" />}
        title={t("settings.preserveCodexOfficialAuthOnSwitch")}
        description={t("settings.preserveCodexOfficialAuthOnSwitchDescription")}
        checked={settings.preserveCodexOfficialAuthOnSwitch ?? false}
        onCheckedChange={(value) =>
          onChange({ preserveCodexOfficialAuthOnSwitch: value })
        }
      />

      <ToggleRow
        icon={<History className="h-4 w-4 text-sky-500" />}
        title={t("settings.unifyCodexSessionHistory")}
        description={t("settings.unifyCodexSessionHistoryDescription")}
        checked={settings.unifyCodexSessionHistory ?? false}
        onCheckedChange={handleUnifyHistoryChange}
      />

      <CodexOfficialContextSettings
        enabled={settings.codexOfficialContext1M ?? false}
        onChange={async (enabled) =>
          await onChange({ codexOfficialContext1M: enabled })
        }
      />

      <ConfirmDialog
        isOpen={showEnableConfirm}
        title={t("confirm.unifyCodexHistory.title")}
        message={t("confirm.unifyCodexHistory.message")}
        checkboxLabel={t("confirm.unifyCodexHistory.migrateExisting")}
        confirmText={t("confirm.unifyCodexHistory.confirm")}
        onConfirm={handleEnableConfirm}
        onCancel={() => setShowEnableConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showDisableConfirm}
        title={t("confirm.unifyCodexHistoryOff.title")}
        message={t("confirm.unifyCodexHistoryOff.message")}
        checkboxLabel={
          showRestoreOption
            ? t("confirm.unifyCodexHistoryOff.restoreBackup")
            : undefined
        }
        checkboxDefaultChecked
        confirmText={t("confirm.unifyCodexHistoryOff.confirm")}
        onConfirm={(restoreBackup) => void handleDisableConfirm(restoreBackup)}
        onCancel={() => setShowDisableConfirm(false)}
      />
    </section>
  );
}

function CodexOfficialContextSettings({
  enabled,
  onChange,
}: {
  enabled: boolean;
  onChange: (enabled: boolean) => Promise<void | boolean>;
}) {
  const { t } = useTranslation();
  const [limits, setLimits] = useState<
    Awaited<ReturnType<typeof settingsApi.getCodexOfficialContextLimits>>
  >([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void settingsApi
      .getCodexOfficialContextLimits(enabled)
      .then((next) => {
        if (active) setLimits(next);
      })
      .catch(() => {
        if (active) setError(t("settings.codexOfficialContextLoadFailed"));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [enabled, t]);

  const select = async (next: boolean) => {
    if (next === enabled || saving) return;
    setSaving(true);
    setError("");
    let saved: void | boolean;
    try {
      saved = await onChange(next);
    } catch {
      setError(t("settings.saveFailedGeneric"));
      setSaving(false);
      return;
    }
    if (saved === false) {
      setError(t("settings.saveFailedGeneric"));
      setSaving(false);
      return;
    }

    try {
      const refreshed = await settingsApi.getCodexOfficialContextLimits(next);
      setLimits(refreshed);
    } catch {
      setError(t("settings.codexOfficialContextLoadFailed"));
    } finally {
      setSaving(false);
    }
  };

  const format = (value: number | null) =>
    value == null
      ? t("settings.codexOfficialContextUnknown")
      : value.toLocaleString();

  return (
    <section className="space-y-3 border-t border-border/40 pt-4">
      <div>
        <h4 className="text-sm font-medium text-foreground">
          {t("settings.codexOfficialContextTitle")}
        </h4>
        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
          {t("settings.codexOfficialContextDescription")}
        </p>
      </div>
      <div
        className="inline-flex rounded-md border border-border p-0.5"
        role="group"
        aria-label={t("settings.codexOfficialContextTitle")}
      >
        <Button
          type="button"
          size="sm"
          variant={!enabled ? "default" : "ghost"}
          aria-pressed={!enabled}
          disabled={saving}
          onClick={() => void select(false)}
        >
          {t("settings.codexOfficialContextDefault")}
        </Button>
        <Button
          type="button"
          size="sm"
          variant={enabled ? "default" : "ghost"}
          aria-pressed={enabled}
          disabled={saving}
          onClick={() => void select(true)}
        >
          {t("settings.codexOfficialContext1M")}
        </Button>
      </div>
      {loading ? (
        <p role="status" className="text-xs text-muted-foreground">
          {t("settings.codexOfficialContextLoading")}
        </p>
      ) : limits.length > 0 ? (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <caption className="pb-1 text-left text-muted-foreground">
              {t("settings.codexOfficialContextCalculated")}
            </caption>
            <thead className="text-muted-foreground">
              <tr>
                <th className="py-1 pr-3 font-medium">
                  {t("settings.codexOfficialContextModel")}
                </th>
                <th className="py-1 pr-3 font-medium">
                  {t("settings.codexOfficialContextDefaultWindow")}
                </th>
                <th className="py-1 pr-3 font-medium">
                  {t("settings.codexOfficialContextRequested")}
                </th>
                <th className="py-1 pr-3 font-medium">
                  {t("settings.codexOfficialContextWindow")}
                </th>
                <th className="py-1 font-medium">
                  {t("settings.codexOfficialContextCompact")}
                </th>
              </tr>
            </thead>
            <tbody>
              {limits.map((item) => (
                <tr key={item.model} className="border-t border-border/40">
                  <td className="py-1.5 pr-3 font-mono">{item.model}</td>
                  <td className="py-1.5 pr-3">
                    {format(item.defaultContextWindow)}
                  </td>
                  <td className="py-1.5 pr-3">
                    {format(item.requestedContextWindow)}
                  </td>
                  <td className="py-1.5 pr-3">
                    {format(item.effectiveContextWindow)}
                    {item.maxContextWindow != null && (
                      <span className="ml-1 text-muted-foreground">
                        /{" "}
                        {t("settings.codexOfficialContextMax", {
                          value: format(item.maxContextWindow),
                        })}
                      </span>
                    )}
                  </td>
                  <td className="py-1.5">
                    {format(item.autoCompactTokenLimit)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
      {error && (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      )}
      <Button
        type="button"
        size="sm"
        variant="ghost"
        disabled={saving || !enabled}
        onClick={() => void select(false)}
      >
        <RotateCcw className="mr-1.5 h-3.5 w-3.5" />
        {t("settings.codexOfficialContextRestore")}
      </Button>
    </section>
  );
}
