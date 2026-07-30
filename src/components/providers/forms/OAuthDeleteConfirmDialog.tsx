import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export type OAuthDeleteTarget =
  | { kind: "account"; accountId: string; label: string }
  | { kind: "all" };

interface OAuthDeleteConfirmDialogProps {
  target: OAuthDeleteTarget | null;
  providerLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function OAuthDeleteConfirmDialog({
  target,
  providerLabel,
  onConfirm,
  onCancel,
}: OAuthDeleteConfirmDialogProps) {
  const { t } = useTranslation();
  const removingAll = target?.kind === "all";
  const accountLabel = target?.kind === "account" ? target.label : "";

  return (
    <ConfirmDialog
      isOpen={target !== null}
      title={
        removingAll
          ? t("oauthDelete.allTitle", {
              provider: providerLabel,
              defaultValue: `移除所有 ${providerLabel} 账号？`,
            })
          : t("oauthDelete.accountTitle", {
              defaultValue: "移除这个账号？",
            })
      }
      message={
        removingAll
          ? t("oauthDelete.allMessage", {
              provider: providerLabel,
              defaultValue: `将移除本机保存的所有 ${providerLabel} 登录凭据。此操作无法撤销。`,
            })
          : t("oauthDelete.accountMessage", {
              account: accountLabel,
              defaultValue: `将移除账号 ${accountLabel} 的本地登录凭据。此操作无法撤销。`,
            })
      }
      confirmText={
        removingAll
          ? t("oauthDelete.removeAll", { defaultValue: "移除全部" })
          : t("oauthDelete.removeAccount", { defaultValue: "移除账号" })
      }
      cancelText={t("common.cancel", { defaultValue: "取消" })}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
