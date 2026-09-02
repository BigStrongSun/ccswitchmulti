import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
  Variable,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ToggleRow } from "@/components/ui/toggle-row";
import { settingsApi } from "@/lib/api/settings";
import type {
  EnvInjectionSettings as EnvInjectionSettingsValue,
  EnvInjectionSyncReport,
  EnvInjectionTargetSyncStatus,
} from "@/types";

export interface EnvInjectionSettingsProps {
  value?: EnvInjectionSettingsValue;
  onChange: (
    value: EnvInjectionSettingsValue,
  ) =>
    | void
    | EnvInjectionSyncReport
    | null
    | Promise<void | EnvInjectionSyncReport | null>;
}

interface EnvRow {
  id: string;
  key: string;
  value: string;
}

const isValidKey = (key: string) =>
  key.length > 0 &&
  !key.includes("=") &&
  !key.includes("\u0000") &&
  !key.includes("\n") &&
  !key.includes("\r");

const toRows = (variables: Record<string, string>): EnvRow[] =>
  Object.entries(variables).map(([key, value], index) => ({
    id: `${index}-${key}`,
    key,
    value,
  }));

const rowsToVariables = (rows: EnvRow[]): Record<string, string> => {
  const variables: Record<string, string> = {};
  for (const row of rows) {
    const key = row.key.trim();
    if (isValidKey(key)) variables[key] = row.value;
  }
  return variables;
};

export function EnvInjectionSettings({
  value,
  onChange,
}: EnvInjectionSettingsProps) {
  const { t } = useTranslation();
  const normalized = useMemo<EnvInjectionSettingsValue>(
    () => ({
      enabled: value?.enabled ?? false,
      targets: {
        claude: value?.targets?.claude ?? true,
        codex: value?.targets?.codex ?? true,
      },
      variables: value?.variables ?? {},
    }),
    [value],
  );
  const variablesSignature = JSON.stringify(normalized.variables);
  const [rows, setRows] = useState(() => toRows(normalized.variables));
  const [draftKey, setDraftKey] = useState("");
  const [draftValue, setDraftValue] = useState("");
  const [validationError, setValidationError] = useState<string | null>(null);
  const [syncReport, setSyncReport] = useState<EnvInjectionSyncReport | null>(
    null,
  );
  const [isSyncing, setIsSyncing] = useState(false);

  useEffect(() => {
    setRows(toRows(JSON.parse(variablesSignature) as Record<string, string>));
  }, [variablesSignature]);

  useEffect(() => {
    let cancelled = false;
    setIsSyncing(true);
    void settingsApi
      .inspectEnvInjectionStatus()
      .then((report) => {
        if (!cancelled) setSyncReport(report);
      })
      .catch((error) => {
        if (!cancelled) {
          console.warn(
            "[EnvInjectionSettings] status inspection failed",
            error,
          );
          setSyncReport(null);
        }
      })
      .finally(() => {
        if (!cancelled) setIsSyncing(false);
      });
    return () => {
      cancelled = true;
    };
  }, [
    normalized.enabled,
    normalized.targets.claude,
    normalized.targets.codex,
    variablesSignature,
  ]);

  const save = useCallback(
    async (next: EnvInjectionSettingsValue) => {
      setIsSyncing(true);
      try {
        const report = await onChange(next);
        if (report) setSyncReport(report);
      } finally {
        setIsSyncing(false);
      }
    },
    [onChange],
  );

  const commitRows = (nextRows: EnvRow[]) => {
    setRows(nextRows);
    void save({ ...normalized, variables: rowsToVariables(nextRows) });
  };

  const addRow = () => {
    const key = draftKey.trim();
    if (!isValidKey(key)) {
      setValidationError(
        t("settings.envInjection.invalidKey", {
          defaultValue: "变量名不能为空，且不能包含 =、换行或 NUL。",
        }),
      );
      return;
    }
    if (rows.some((row) => row.key.trim() === key)) {
      setValidationError(
        t("settings.envInjection.duplicateKey", {
          defaultValue: "变量名已存在。",
        }),
      );
      return;
    }
    setValidationError(null);
    setDraftKey("");
    setDraftValue("");
    commitRows([
      ...rows,
      { id: `${Date.now()}-${key}`, key, value: draftValue },
    ]);
  };

  const retry = async () => {
    setIsSyncing(true);
    try {
      setSyncReport(await settingsApi.retryEnvInjectionSync());
    } catch (error) {
      console.warn("[EnvInjectionSettings] retry failed", error);
    } finally {
      setIsSyncing(false);
    }
  };

  const showRetry =
    syncReport?.state === "partial" ||
    syncReport?.state === "failed" ||
    syncReport?.claude.state === "pending" ||
    syncReport?.codex.state === "pending";

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.envInjection.title", { defaultValue: "环境变量注入" })}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.envInjection.description", {
            defaultValue:
              "把变量写进各 CLI 自己的配置文件，不改系统环境，也不依赖本地代理。",
          })}
        </p>
      </header>

      <ToggleRow
        icon={<Variable className="h-4 w-4 text-violet-500" />}
        title={t("settings.envInjection.enable", {
          defaultValue: "启用环境变量注入",
        })}
        description={t("settings.envInjection.enableDescription", {
          defaultValue: "关闭后只移除有 CCSM 所有权记录且未被用户修改的值。",
        })}
        checked={normalized.enabled}
        onCheckedChange={(enabled) => void save({ ...normalized, enabled })}
      />

      {normalized.enabled ? (
        <div className="space-y-4 rounded-xl border border-border bg-card/50 p-4">
          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("settings.envInjection.targets", { defaultValue: "生效范围" })}
            </p>
            <div className="flex flex-wrap gap-2">
              <TargetButton
                active={normalized.targets.claude}
                onClick={() =>
                  void save({
                    ...normalized,
                    targets: {
                      ...normalized.targets,
                      claude: !normalized.targets.claude,
                    },
                  })
                }
              >
                Claude Code
              </TargetButton>
              <TargetButton
                active={normalized.targets.codex}
                onClick={() =>
                  void save({
                    ...normalized,
                    targets: {
                      ...normalized.targets,
                      codex: !normalized.targets.codex,
                    },
                  })
                }
              >
                Codex
              </TargetButton>
            </div>
            <p className="text-xs text-muted-foreground">
              {t("settings.envInjection.scopeHint", {
                defaultValue:
                  "Claude Code：主进程和子进程；Codex：仅其派生的命令子进程。Gemini 本版没有受管注入通道。",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("settings.envInjection.variables", { defaultValue: "变量" })}
            </p>
            {rows.map((row) => (
              <div key={row.id} className="flex items-center gap-2">
                <Input
                  aria-label={t("settings.envInjection.keyPlaceholder", {
                    defaultValue: "变量名",
                  })}
                  className="h-8 flex-[2] font-mono text-xs"
                  value={row.key}
                  onChange={(event) =>
                    setRows((current) =>
                      current.map((item) =>
                        item.id === row.id
                          ? { ...item, key: event.target.value }
                          : item,
                      ),
                    )
                  }
                  onBlur={() => commitRows(rows)}
                />
                <Input
                  aria-label={t("settings.envInjection.valuePlaceholder", {
                    defaultValue: "值",
                  })}
                  className="h-8 flex-[3] font-mono text-xs"
                  value={row.value}
                  onChange={(event) =>
                    setRows((current) =>
                      current.map((item) =>
                        item.id === row.id
                          ? { ...item, value: event.target.value }
                          : item,
                      ),
                    )
                  }
                  onBlur={() => commitRows(rows)}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  aria-label={t("common.delete", { defaultValue: "删除" })}
                  onClick={() =>
                    commitRows(rows.filter((item) => item.id !== row.id))
                  }
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}

            <div className="flex items-center gap-2">
              <Input
                aria-label={t("settings.envInjection.keyPlaceholder", {
                  defaultValue: "变量名",
                })}
                className="h-8 flex-[2] font-mono text-xs"
                value={draftKey}
                onChange={(event) => setDraftKey(event.target.value)}
              />
              <Input
                aria-label={t("settings.envInjection.valuePlaceholder", {
                  defaultValue: "值",
                })}
                className="h-8 flex-[3] font-mono text-xs"
                value={draftValue}
                onChange={(event) => setDraftValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") addRow();
                }}
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-8 w-8 shrink-0"
                aria-label={t("settings.envInjection.add", {
                  defaultValue: "添加变量",
                })}
                onClick={addRow}
              >
                <Plus className="h-4 w-4" />
              </Button>
            </div>
            {validationError ? (
              <p className="text-xs text-destructive">{validationError}</p>
            ) : null}
          </div>

          <SyncStatus report={syncReport} />

          {showRetry ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => void retry()}
            >
              {isSyncing ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="mr-2 h-4 w-4" />
              )}
              {t("settings.envInjection.retry", { defaultValue: "重试同步" })}
            </Button>
          ) : null}

          {syncReport?.codexIncludeAllowlist ? (
            <div className="flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                {t("settings.envInjection.codexIncludeAllowlistWarning", {
                  defaultValue:
                    "Codex 的 include_only 白名单可能过滤这些变量，请把所需变量加入白名单。",
                })}
              </span>
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

function SyncStatus({ report }: { report: EnvInjectionSyncReport | null }) {
  const { t } = useTranslation();
  if (!report) return null;
  const headline = {
    disabled: t("settings.envInjection.statusDisabled", {
      defaultValue: "尚未启用",
    }),
    synced: t("settings.envInjection.statusSynced", {
      defaultValue: "所有已启用目标均已同步",
    }),
    warning: t("settings.envInjection.statusWarning", {
      defaultValue: "部分变量保持用户所有，未由 CCSM 覆盖",
    }),
    partial: t("settings.envInjection.statusPartial", {
      defaultValue: "设置已保存，但部分 CLI 尚未同步",
    }),
    failed: t("settings.envInjection.statusFailed", {
      defaultValue: "CLI 同步失败，请重试",
    }),
  }[report.state];

  return (
    <div className="space-y-2 rounded-lg border border-border p-3 text-xs">
      <p className="font-medium">{headline}</p>
      <TargetStatus name="Claude Code" status={report.claude} />
      <TargetStatus name="Codex" status={report.codex} />
    </div>
  );
}

function TargetStatus({
  name,
  status,
}: {
  name: string;
  status: EnvInjectionTargetSyncStatus;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1 text-muted-foreground">
      <p>{name}</p>
      {status.conflictedKeys.map((key) => (
        <p
          key={`${name}-${key}`}
          className="text-amber-700 dark:text-amber-300"
        >
          {t("settings.envInjection.userOwnedConflict", {
            defaultValue: `${key} 已存在并保持用户所有，CCSM 不会覆盖或删除它`,
            key,
          })}
        </p>
      ))}
      {status.error ? <p className="text-destructive">{status.error}</p> : null}
      {status.rollbackError ? (
        <p className="text-destructive">
          {t("settings.envInjection.rollbackFailed", {
            defaultValue: `回滚失败：${status.rollbackError}`,
            error: status.rollbackError,
          })}
        </p>
      ) : null}
    </div>
  );
}

function TargetButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Button
      type="button"
      size="sm"
      variant={active ? "default" : "ghost"}
      className="min-w-[110px]"
      onClick={onClick}
    >
      {children}
    </Button>
  );
}
