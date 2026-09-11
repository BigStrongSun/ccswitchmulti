import { ExternalLink, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import type { ManagedAuthAccount } from "@/lib/api/auth";
import type { CodexOfficialAuthMigrationStatus } from "@/lib/api/providers";
import type { CodexOfficialAuthConfig, CodexOfficialAuthMode } from "@/types";

interface CodexOfficialAuthSectionProps {
  value: CodexOfficialAuthConfig;
  accounts: ManagedAuthAccount[];
  migrationStatus?: CodexOfficialAuthMigrationStatus;
  onChange: (value: CodexOfficialAuthConfig) => void;
  onOpenAuthCenter: () => void;
}

export function validateCodexOfficialAuthSelection(
  value: CodexOfficialAuthConfig,
  accounts: ManagedAuthAccount[],
): string | null {
  if (value.mode !== "managed_oauth") return null;
  const accountId = value.accountId?.trim();
  const usable = accounts.some(
    (account) => account.id === accountId && !account.requires_reauth,
  );
  return usable ? null : "所选 CCSM OAuth 账号不存在或需要重新登录";
}

export function CodexOfficialAuthSection({
  value,
  accounts,
  migrationStatus,
  onChange,
  onOpenAuthCenter,
}: CodexOfficialAuthSectionProps) {
  const { t } = useTranslation();
  const usableAccounts = accounts.filter((account) => !account.requires_reauth);
  const validationError = validateCodexOfficialAuthSelection(value, accounts);

  const handleModeChange = (mode: CodexOfficialAuthMode) => {
    if (mode !== "managed_oauth") {
      onChange({ mode });
      return;
    }
    const currentAccount = usableAccounts.find(
      (account) => account.id === value.accountId,
    );
    const preferredAccount =
      currentAccount ??
      usableAccounts.find((account) => account.is_default) ??
      usableAccounts[0];
    onChange({
      mode: "managed_oauth",
      ...(preferredAccount ? { accountId: preferredAccount.id } : {}),
    });
  };

  return (
    <section
      id="codex-official-auth-settings"
      className="space-y-3 rounded-lg border border-blue-200 bg-blue-50/60 p-4 dark:border-blue-800/60 dark:bg-blue-950/20"
    >
      <div className="flex items-start gap-3">
        <ShieldCheck className="mt-0.5 h-5 w-5 text-blue-600 dark:text-blue-400" />
        <div>
          <h3 className="font-semibold">
            {t("codexOfficialAuth.title", {
              defaultValue: "OpenAI Official 认证设置",
            })}
          </h3>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {t("codexOfficialAuth.description", {
              defaultValue:
                "此处是官方模型认证的唯一配置入口。直接使用 OpenAI Official 与 MultiRouter 官方路由会继承同一设置。",
            })}
          </p>
        </div>
      </div>

      {migrationStatus?.state === "conflict" ? (
        <div className="rounded-md border border-amber-300 bg-amber-50 p-3 text-xs leading-5 text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/30 dark:text-amber-100">
          {t("codexOfficialAuth.conflict", {
            routers: migrationStatus.conflictingRouterIds.join("、"),
            defaultValue:
              "检测到旧 MultiRouter 的官方认证配置不一致：{{routers}}。请选择当前应采用的认证方式并保存 OpenAI Official；旧配置会保留到选择明确后再迁移。",
          })}
        </div>
      ) : null}

      <div className="grid gap-2">
        <label
          htmlFor="codex-official-auth-mode"
          className="text-sm font-medium"
        >
          {t("codexOfficialAuth.modeLabel", { defaultValue: "官方认证方式" })}
        </label>
        <select
          id="codex-official-auth-mode"
          aria-label={t("codexOfficialAuth.modeLabel", {
            defaultValue: "官方认证方式",
          })}
          value={value.mode}
          onChange={(event) =>
            handleModeChange(event.target.value as CodexOfficialAuthMode)
          }
          className="h-10 rounded-md border bg-background px-3 text-sm"
        >
          <option value="desktop_current_login">
            {t("codexOfficialAuth.desktopOption", {
              defaultValue: "Codex Desktop 当前登录",
            })}
          </option>
          <option value="managed_oauth">
            {t("codexOfficialAuth.managedOption", {
              defaultValue: "CCSM OAuth 固定账号",
            })}
          </option>
          <option value="account_pool">
            {t("codexOfficialAuth.poolOption", {
              defaultValue: "OAuth 账号池",
            })}
          </option>
        </select>
      </div>

      {value.mode === "managed_oauth" ? (
        <div className="grid gap-2">
          <label
            htmlFor="codex-official-auth-account"
            className="text-sm font-medium"
          >
            {t("codexOfficialAuth.managedAccountLabel", {
              defaultValue: "CCSM OAuth 固定账号",
            })}
          </label>
          <select
            id="codex-official-auth-account"
            aria-label={t("codexOfficialAuth.managedAccountLabel", {
              defaultValue: "CCSM OAuth 固定账号",
            })}
            value={value.accountId ?? ""}
            onChange={(event) =>
              onChange({ mode: "managed_oauth", accountId: event.target.value })
            }
            className="h-10 rounded-md border bg-background px-3 text-sm"
          >
            <option value="" disabled>
              {t("codexOfficialAuth.selectAccount", {
                defaultValue: "请选择已登录账号",
              })}
            </option>
            {accounts.map((account) => (
              <option
                key={account.id}
                value={account.id}
                disabled={account.requires_reauth}
              >
                {account.login}
                {account.requires_reauth
                  ? t("codexOfficialAuth.reauthMarker", {
                      defaultValue: "（需要重新登录）",
                    })
                  : account.is_default
                    ? t("codexOfficialAuth.defaultMarker", {
                        defaultValue: "（默认）",
                      })
                    : ""}
              </option>
            ))}
          </select>
          {validationError ? (
            <p className="text-xs text-red-600 dark:text-red-400">
              {t("codexOfficialAuth.accountUnavailable", {
                defaultValue: validationError,
              })}
            </p>
          ) : null}
        </div>
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border bg-background/70 p-3">
        <p className="min-w-0 flex-1 text-xs leading-5 text-muted-foreground">
          {value.mode === "account_pool"
            ? t("codexOfficialAuth.poolHint", {
                defaultValue:
                  "账号池可由 OpenAI Official 直接使用，无需启用 MultiRouter；MultiRouter 中的官方模型会继承同一设置。成员顺序、保留额度和冷却策略仍在认证中心维护。",
              })
            : t("codexOfficialAuth.authCenterHint", {
                defaultValue:
                  "账号登录、重新认证和账号池成员策略在认证中心维护；这里仅保存不含 Token 的账号引用。",
              })}
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={onOpenAuthCenter}
        >
          {t("codexOfficialAuth.openAuthCenter", {
            defaultValue: "前往认证中心",
          })}
          <ExternalLink className="h-4 w-4" />
        </Button>
      </div>
    </section>
  );
}
