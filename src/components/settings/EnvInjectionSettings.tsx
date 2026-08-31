import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Plus, Trash2, Variable } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ToggleRow } from "@/components/ui/toggle-row";
import { settingsApi } from "@/lib/api/settings";
import type {
  EnvInjectionConflicts,
  EnvInjectionSettings as EnvInjectionSettingsValue,
} from "@/types";

export interface EnvInjectionSettingsProps {
  value?: EnvInjectionSettingsValue;
  onChange: (value: EnvInjectionSettingsValue) => void;
}

interface EnvRow {
  id: string;
  key: string;
  value: string;
}

/**
 * 键名不能为空、不能含 `=`、NUL 或换行，否则会写出损坏的配置。
 * 后端 `is_valid_env_key` 有同样一套校验，这里提前拦一次给即时反馈。
 */
const isValidKey = (key: string) =>
  key.length > 0 &&
  !key.includes("=") &&
  !key.includes("\u0000") &&
  !key.includes("\n");

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
    if (!isValidKey(key)) continue;
    variables[key] = row.value;
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

  const [rows, setRows] = useState<EnvRow[]>(() =>
    toRows(normalized.variables),
  );
  const [draftKey, setDraftKey] = useState("");
  const [draftValue, setDraftValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [conflicts, setConflicts] = useState<EnvInjectionConflicts | null>(
    null,
  );

  // 外部改动（配置导入、云同步、重置）后重新对齐本地草稿。
  // 用序列化后的签名做依赖，避免对象引用变化导致每轮渲染都重置输入框。
  const variablesSignature = JSON.stringify(normalized.variables);
  useEffect(() => {
    setRows(toRows(JSON.parse(variablesSignature) as Record<string, string>));
  }, [variablesSignature]);

  // 冲突只读提示，失败就当没有冲突，不阻塞 UI
  useEffect(() => {
    if (!normalized.enabled) {
      setConflicts(null);
      return;
    }
    let cancelled = false;
    void settingsApi.inspectEnvInjectionConflicts().then((result) => {
      if (!cancelled) setConflicts(result);
    });
    return () => {
      cancelled = true;
    };
  }, [normalized.enabled, variablesSignature]);

  const commit = (nextRows: EnvRow[]) => {
    setRows(nextRows);
    onChange({ ...normalized, variables: rowsToVariables(nextRows) });
  };

  const updateRow = (id: string, patch: Partial<EnvRow>) => {
    setRows((current) =>
      current.map((row) => (row.id === id ? { ...row, ...patch } : row)),
    );
  };

  const removeRow = (id: string) => {
    commit(rows.filter((row) => row.id !== id));
  };

  const addRow = () => {
    const key = draftKey.trim();
    if (!isValidKey(key)) {
      setError(t("settings.envInjection.invalidKey"));
      return;
    }
    if (rows.some((row) => row.key.trim() === key)) {
      setError(t("settings.envInjection.duplicateKey"));
      return;
    }
    setError(null);
    setDraftKey("");
    setDraftValue("");
    commit([...rows, { id: `${Date.now()}-${key}`, key, value: draftValue }]);
  };

  const setEnabled = (enabled: boolean) => {
    setError(null);
    onChange({ ...normalized, enabled });
  };

  const setTarget = (target: "claude" | "codex", checked: boolean) => {
    onChange({
      ...normalized,
      targets: { ...normalized.targets, [target]: checked },
    });
  };

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.envInjection.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.envInjection.description")}
        </p>
      </header>

      <ToggleRow
        icon={<Variable className="h-4 w-4 text-violet-500" />}
        title={t("settings.envInjection.enable")}
        description={t("settings.envInjection.enableDescription")}
        checked={normalized.enabled}
        onCheckedChange={setEnabled}
      />

      {normalized.enabled ? (
        <div className="space-y-4 rounded-xl border border-border bg-card/50 p-4">
          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("settings.envInjection.targets")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("settings.envInjection.targetsDescription")}
            </p>
            <div className="flex flex-wrap gap-1">
              <TargetButton
                active={normalized.targets.claude}
                onClick={() => setTarget("claude", !normalized.targets.claude)}
              >
                {t("settings.envInjection.claude")}
              </TargetButton>
              <TargetButton
                active={normalized.targets.codex}
                onClick={() => setTarget("codex", !normalized.targets.codex)}
              >
                {t("settings.envInjection.codex")}
              </TargetButton>
            </div>
            <p className="text-xs text-muted-foreground">
              {t("settings.envInjection.geminiUnsupported")}
            </p>
          </div>

          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("settings.envInjection.variables")}
            </p>

            {rows.length === 0 ? (
              <p className="text-xs text-muted-foreground">
                {t("settings.envInjection.empty")}
              </p>
            ) : (
              <div className="space-y-2">
                {rows.map((row) => (
                  <div key={row.id} className="flex items-center gap-2">
                    <Input
                      value={row.key}
                      aria-label={t("settings.envInjection.keyPlaceholder")}
                      placeholder={t("settings.envInjection.keyPlaceholder")}
                      className="h-8 flex-[2] font-mono text-xs"
                      onChange={(event) =>
                        updateRow(row.id, { key: event.target.value })
                      }
                      onBlur={() => commit(rows)}
                    />
                    <Input
                      value={row.value}
                      aria-label={t("settings.envInjection.valuePlaceholder")}
                      placeholder={t("settings.envInjection.valuePlaceholder")}
                      className="h-8 flex-[3] font-mono text-xs"
                      onChange={(event) =>
                        updateRow(row.id, { value: event.target.value })
                      }
                      onBlur={() => commit(rows)}
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                      aria-label={t("common.delete")}
                      onClick={() => removeRow(row.id)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            )}

            <div className="flex items-center gap-2">
              <Input
                value={draftKey}
                aria-label={t("settings.envInjection.keyPlaceholder")}
                placeholder={t("settings.envInjection.keyPlaceholder")}
                className="h-8 flex-[2] font-mono text-xs"
                onChange={(event) => setDraftKey(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") addRow();
                }}
              />
              <Input
                value={draftValue}
                aria-label={t("settings.envInjection.valuePlaceholder")}
                placeholder={t("settings.envInjection.valuePlaceholder")}
                className="h-8 flex-[3] font-mono text-xs"
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
                aria-label={t("settings.envInjection.add")}
                onClick={addRow}
              >
                <Plus className="h-4 w-4" />
              </Button>
            </div>

            {error ? <p className="text-xs text-destructive">{error}</p> : null}
          </div>

          <p className="text-xs text-muted-foreground">
            {t("settings.envInjection.mergeHint")}
          </p>

          {conflicts?.codexIncludeAllowlist ? (
            <div className="flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                {t("settings.envInjection.codexIncludeAllowlistWarning")}
              </span>
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

interface TargetButtonProps {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function TargetButton({ active, onClick, children }: TargetButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={
        active
          ? "min-w-[110px] shadow-sm"
          : "min-w-[110px] text-muted-foreground hover:bg-muted hover:text-foreground"
      }
    >
      {children}
    </Button>
  );
}
