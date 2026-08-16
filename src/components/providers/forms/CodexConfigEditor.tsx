import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight } from "lucide-react";
import { CodexAuthSection, CodexConfigSection } from "./CodexConfigSections";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";

interface CodexConfigEditorProps {
  authValue: string;

  configValue: string;

  providerName?: string;

  showRemoteCompaction?: boolean;

  isProxyTakeover?: boolean;

  onAuthChange: (value: string) => void;

  onConfigChange: (value: string) => void;

  onAuthBlur?: () => void;

  useCommonConfig: boolean;

  onCommonConfigToggle: (checked: boolean) => void | Promise<void>;

  commonConfigError: string;

  authError: string;

  configError: string; // config.toml 错误提示
}

const CodexConfigEditor: React.FC<CodexConfigEditorProps> = ({
  authValue,
  configValue,
  providerName,
  showRemoteCompaction,
  isProxyTakeover = false,
  onAuthChange,
  onConfigChange,
  onAuthBlur,
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigError,
  authError,
  configError,
}) => {
  const { t } = useTranslation();
  const [isExpertOpen, setIsExpertOpen] = useState(false);

  return (
    <Collapsible
      open={isExpertOpen}
      onOpenChange={setIsExpertOpen}
      className="rounded-lg border border-border-default p-4"
    >
      <CollapsibleTrigger asChild>
        <Button
          type="button"
          variant={null}
          size="sm"
          className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
        >
          {isExpertOpen ? (
            <ChevronDown className="h-4 w-4" />
          ) : (
            <ChevronRight className="h-4 w-4" />
          )}
          专家配置
        </Button>
      </CollapsibleTrigger>
      {!isExpertOpen && (
        <p className="mt-1 ml-1 text-xs text-muted-foreground">
          手工编辑 auth.json、config.toml、远程压缩和 Provider
          级通用配置应用；正常接入无需展开。
        </p>
      )}
      <CollapsibleContent className="space-y-6 pt-4">
        {isProxyTakeover && (
          <div className="rounded-lg border border-amber-200 bg-amber-50 p-3 dark:border-amber-700 dark:bg-amber-900/20">
            <p className="text-xs text-amber-600 dark:text-amber-400">
              {t("codexConfig.proxyTakeoverStorageNotice")}
            </p>
          </div>
        )}
        <CodexAuthSection
          value={authValue}
          onChange={onAuthChange}
          onBlur={onAuthBlur}
          error={authError}
          isProxyTakeover={isProxyTakeover}
        />
        <CodexConfigSection
          value={configValue}
          onChange={onConfigChange}
          providerName={providerName}
          showRemoteCompaction={showRemoteCompaction}
          useCommonConfig={useCommonConfig}
          onCommonConfigToggle={onCommonConfigToggle}
          commonConfigError={commonConfigError}
          configError={configError}
          isProxyTakeover={isProxyTakeover}
        />
      </CollapsibleContent>
    </Collapsible>
  );
};

export default CodexConfigEditor;
