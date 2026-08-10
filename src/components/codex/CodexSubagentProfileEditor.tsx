import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { codexSubagentV2Api } from "@/lib/api/codexSubagentV2";
import type { Provider } from "@/types";
import {
  createDefaultCodexSubagentV2Config,
  type CodexSubagentExplicitReasoningEffort,
  type CodexSubagentProfilePreview,
  type CodexSubagentProfileStatus,
  type CodexSubagentProfileStatuses,
  type CodexSubagentTaskStrength,
  type CodexSubagentV2Config,
  type CodexSubagentV2Profile,
} from "@/types/codexSubagentV2";

const TASK_STRENGTHS: Array<{
  value: CodexSubagentTaskStrength;
  label: string;
}> = [
  { value: "long_context_reading", label: "长上下文阅读" },
  { value: "repository_exploration", label: "仓库探索" },
  { value: "evidence_collection", label: "证据收集" },
  { value: "summarization", label: "总结归纳" },
  { value: "complex_debugging", label: "复杂调试" },
  { value: "architecture_design", label: "架构设计" },
  { value: "bounded_implementation", label: "有限实现" },
  { value: "complex_implementation", label: "复杂实现" },
  { value: "testing", label: "测试验证" },
  { value: "high_risk_review", label: "高风险审查" },
];

const PROFILE_TITLES: Record<string, string> = {
  "deepseek-v4-flash": "DeepSeek V4 Flash 子 Agent 能力",
  "deepseek-v4-pro": "DeepSeek V4 Pro 子 Agent 能力",
};

const EXPLICIT_REASONING_EFFORTS = new Set(["low", "medium", "high", "xhigh"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isUsableProfile(value: unknown): value is CodexSubagentV2Profile {
  if (!isRecord(value) || typeof value.model !== "string") return false;
  if (typeof value.enabled !== "boolean" || !isRecord(value.questionnaire)) {
    return false;
  }
  const questionnaire = value.questionnaire;
  if (
    !(
      Array.isArray(questionnaire.taskStrengths) &&
      questionnaire.taskStrengths.every(
        (strength) => typeof strength === "string",
      ) &&
      typeof questionnaire.optimization === "string" &&
      typeof questionnaire.writeScope === "string" &&
      typeof questionnaire.preference === "string" &&
      typeof questionnaire.reasoningEffort === "string"
    )
  ) {
    return false;
  }
  if (value.overrides === undefined) return true;
  if (!isRecord(value.overrides)) return false;
  const overrides = value.overrides;
  return (
    (overrides.roleName === undefined ||
      typeof overrides.roleName === "string") &&
    (overrides.description === undefined ||
      typeof overrides.description === "string") &&
    (overrides.developerInstructions === undefined ||
      typeof overrides.developerInstructions === "string") &&
    (overrides.nicknameCandidates === undefined ||
      (Array.isArray(overrides.nicknameCandidates) &&
        overrides.nicknameCandidates.every(
          (nickname) => typeof nickname === "string",
        ))) &&
    (overrides.modelReasoningEffort === undefined ||
      (typeof overrides.modelReasoningEffort === "string" &&
        EXPLICIT_REASONING_EFFORTS.has(overrides.modelReasoningEffort)))
  );
}

function readRawProfiles(
  config: CodexSubagentV2Config,
): Record<string, unknown> {
  return isRecord(config.profiles) ? config.profiles : {};
}

function defaultProfileForModel(model: string): CodexSubagentV2Profile | null {
  const defaults = createDefaultCodexSubagentV2Config().profiles;
  return (
    Object.values(defaults).find((profile) => profile.model === model) ?? null
  );
}

function readPersistedConfig(provider: Provider): CodexSubagentV2Config | null {
  const routing = provider.settingsConfig?.codexRouting;
  if (!isRecord(routing) || !isRecord(routing.subagentV2)) return null;
  return routing.subagentV2 as unknown as CodexSubagentV2Config;
}

function settingsWithConfig(
  provider: Provider,
  config: CodexSubagentV2Config,
): Record<string, unknown> {
  const rawSettings = isRecord(provider.settingsConfig)
    ? provider.settingsConfig
    : {};
  const rawRouting = isRecord(rawSettings.codexRouting)
    ? rawSettings.codexRouting
    : {};
  return {
    ...rawSettings,
    codexRouting: {
      ...rawRouting,
      subagentV2: config,
    },
  };
}

function parseNicknames(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function nicknameError(profiles: Record<string, unknown>) {
  for (const profile of Object.values(profiles)) {
    if (!isUsableProfile(profile)) continue;
    const values = profile.overrides?.nicknameCandidates;
    if (!values) continue;
    const unique = new Set(values);
    if (
      values.length < 1 ||
      values.length > 3 ||
      unique.size !== values.length ||
      values.some((value) => !/^[A-Za-z0-9 _-]+$/.test(value))
    ) {
      return "昵称候选需为 1 至 3 个不重复的 ASCII 字母、数字、空格、短横线或下划线";
    }
  }
  return null;
}

function strengthError(profiles: Record<string, unknown>) {
  return Object.values(profiles).some(
    (profile) =>
      (isUsableProfile(profile) &&
        profile.questionnaire.taskStrengths.length < 1) ||
      (isUsableProfile(profile) &&
        (profile.questionnaire.taskStrengths.length > 5 ||
          new Set(profile.questionnaire.taskStrengths).size !==
            profile.questionnaire.taskStrengths.length)),
  )
    ? "每个模型需选择 1 至 5 项不重复的任务优势"
    : null;
}

export function CodexSubagentProfileEditor({
  provider,
  onPersisted,
}: {
  provider: Provider;
  onPersisted?: (provider: Provider) => void;
}) {
  const queryClient = useQueryClient();
  const persistedConfig = readPersistedConfig(provider);
  const persistedKey = JSON.stringify(persistedConfig);
  const [draft, setDraft] = useState<CodexSubagentV2Config | null>(
    persistedConfig,
  );
  const [previews, setPreviews] = useState<
    Record<string, CodexSubagentProfilePreview>
  >({});
  const [previewErrors, setPreviewErrors] = useState<Record<string, string>>(
    {},
  );
  const [nicknameDrafts, setNicknameDrafts] = useState<Record<string, string>>(
    {},
  );
  const [statuses, setStatuses] = useState<CodexSubagentProfileStatuses | null>(
    null,
  );
  const [statusError, setStatusError] = useState<string | null>(null);
  const [strengthLimitMessage, setStrengthLimitMessage] = useState<
    string | null
  >(null);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    setDraft(readPersistedConfig(provider));
    setSaveMessage(null);
    setSaveError(null);
    setStrengthLimitMessage(null);
    setNicknameDrafts({});
    setStatusError(null);
  }, [provider.id, persistedKey]);

  const draftSettings = useMemo(
    () =>
      draft ? settingsWithConfig(provider, draft) : provider.settingsConfig,
    [draft, provider],
  );
  const draftSettingsKey = JSON.stringify(draftSettings);

  useEffect(() => {
    if (!draft) {
      setStatuses(null);
      setStatusError(null);
      return;
    }
    let ignore = false;
    setStatusError(null);
    codexSubagentV2Api
      .getProfileStatuses(draftSettings)
      .then((result) => {
        if (!ignore) setStatuses(result);
      })
      .catch((error) => {
        if (!ignore) {
          setStatuses(null);
          setStatusError(
            error instanceof Error ? error.message : String(error),
          );
        }
      });
    return () => {
      ignore = true;
    };
  }, [draftSettingsKey]);

  useEffect(() => {
    if (!draft) {
      setPreviews({});
      setPreviewErrors({});
      return;
    }
    let ignore = false;
    const entries = Object.entries(readRawProfiles(draft)).filter(
      (entry): entry is [string, CodexSubagentV2Profile] =>
        isUsableProfile(entry[1]),
    );
    Promise.all(
      entries.map(async ([profileKey, profile]) => {
        try {
          const preview = await codexSubagentV2Api.previewProfile(
            draftSettings,
            profile.model,
            profile,
          );
          return { profileKey, preview } as const;
        } catch (error) {
          return {
            profileKey,
            error: error instanceof Error ? error.message : String(error),
          } as const;
        }
      }),
    ).then((results) => {
      if (ignore) return;
      const nextPreviews: Record<string, CodexSubagentProfilePreview> = {};
      const nextErrors: Record<string, string> = {};
      for (const result of results) {
        if ("preview" in result && result.preview)
          nextPreviews[result.profileKey] = result.preview;
        else nextErrors[result.profileKey] = result.error;
      }
      setPreviews(nextPreviews);
      setPreviewErrors(nextErrors);
    });
    return () => {
      ignore = true;
    };
  }, [draftSettingsKey]);

  async function persist(nextConfig: CodexSubagentV2Config) {
    const nextProvider = await codexSubagentV2Api.updateProviderConfig(
      provider.id,
      nextConfig,
    );
    setDraft(nextConfig);
    onPersisted?.(nextProvider);
    await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
  }

  async function initialize() {
    setIsSaving(true);
    setSaveError(null);
    try {
      await persist(createDefaultCodexSubagentV2Config());
      setSaveMessage("V2 子 Agent 能力配置已初始化");
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsSaving(false);
    }
  }

  function updateProfile(
    profileKey: string,
    updater: (profile: CodexSubagentV2Profile) => CodexSubagentV2Profile,
  ) {
    setSaveMessage(null);
    setSaveError(null);
    setDraft((current) =>
      current
        ? {
            ...current,
            profiles: {
              ...current.profiles,
              [profileKey]: updater(current.profiles[profileKey]),
            },
          }
        : current,
    );
  }

  function repairProfile(profileKey: string) {
    const replacement = defaultProfileForModel(profileKey);
    if (!replacement) {
      setSaveError(`没有可用于 ${profileKey} 的安全默认问卷`);
      return;
    }
    setSaveMessage(null);
    setSaveError(null);
    setDraft((current) =>
      current
        ? {
            ...current,
            profiles: {
              ...readRawProfiles(current),
              [profileKey]: replacement,
            } as Record<string, CodexSubagentV2Profile>,
          }
        : current,
    );
  }

  function setOverride<
    K extends keyof NonNullable<CodexSubagentV2Profile["overrides"]>,
  >(
    profileKey: string,
    key: K,
    value: NonNullable<CodexSubagentV2Profile["overrides"]>[K],
  ) {
    updateProfile(profileKey, (profile) => ({
      ...profile,
      overrides: { ...profile.overrides, [key]: value },
    }));
  }

  function restoreOverride(
    profileKey: string,
    key: keyof NonNullable<CodexSubagentV2Profile["overrides"]>,
  ) {
    if (key === "nicknameCandidates") {
      setNicknameDrafts((current) => {
        const next = { ...current };
        delete next[profileKey];
        return next;
      });
    }
    updateProfile(profileKey, (profile) => {
      const overrides = { ...profile.overrides };
      delete overrides[key];
      return {
        ...profile,
        ...(Object.keys(overrides).length > 0
          ? { overrides }
          : { overrides: undefined }),
      };
    });
  }

  async function save() {
    if (!draft) return;
    const rawProfiles = readRawProfiles(draft);
    const localError = strengthError(rawProfiles) ?? nicknameError(rawProfiles);
    if (localError) {
      setSaveError(localError);
      return;
    }
    setIsSaving(true);
    setSaveError(null);
    setSaveMessage(null);
    try {
      const authoritativeStatuses =
        await codexSubagentV2Api.getProfileStatuses(draftSettings);
      setStatuses(authoritativeStatuses);
      setStatusError(null);
      const blocking = authoritativeStatuses.profiles.filter(
        (profile) =>
          profile.status === "collision" || profile.status === "invalid",
      );
      if (blocking.length > 0) {
        const details = blocking.flatMap((profile) => profile.warnings);
        throw new Error(
          details[0] ??
            blocking
              .map(
                (profile) =>
                  `${profile.profileKey ?? profile.model ?? "V2 profile"}：${profile.status}`,
              )
              .join("；"),
        );
      }
      await persist(draft);
      setSaveMessage("V2 子 Agent 能力配置已保存");
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsSaving(false);
    }
  }

  if (!draft) {
    return (
      <section className="rounded-lg border border-emerald-200 bg-emerald-50/50 p-4 dark:border-emerald-700/50 dark:bg-emerald-950/20">
        <p className="text-sm text-muted-foreground">
          当前方案仍使用兼容的 legacy managed
          roles。初始化后才会持久化显式问卷输入。
        </p>
        <Button className="mt-3" onClick={initialize} disabled={isSaving}>
          初始化 V2 子 Agent 能力配置
        </Button>
        {saveError ? (
          <p className="mt-2 text-sm text-rose-600">{saveError}</p>
        ) : null}
      </section>
    );
  }

  const rawProfileEntries = Object.entries(readRawProfiles(draft));
  const profileEntries = rawProfileEntries.filter(
    (entry): entry is [string, CodexSubagentV2Profile] =>
      isUsableProfile(entry[1]),
  );
  const invalidProfileEntries = rawProfileEntries.filter(
    ([, profile]) => !isUsableProfile(profile),
  );
  const statusByProfileKey = new Map(
    (statuses?.profiles ?? [])
      .filter((status) => status.profileKey)
      .map((status) => [status.profileKey!, status]),
  );
  const unassignedStatuses = (statuses?.profiles ?? []).filter(
    (status) =>
      !status.profileKey ||
      !rawProfileEntries.some(
        ([profileKey]) => profileKey === status.profileKey,
      ),
  );

  return (
    <section className="space-y-4 rounded-lg border border-emerald-200 bg-emerald-50/40 p-4 dark:border-emerald-700/50 dark:bg-emerald-950/15">
      <fieldset disabled={isSaving} className="contents">
        <div>
          <h3 className="text-sm font-semibold">选择策略</h3>
          <label className="mt-2 grid gap-1 text-sm">
            <span>第三方子 Agent 选择策略</span>
            <select
              className="rounded-md border bg-background px-3 py-2"
              value={draft.selectionPolicy}
              onChange={(event) => {
                setSaveMessage(null);
                setSaveError(null);
                setDraft({
                  ...draft,
                  selectionPolicy: event.target
                    .value as CodexSubagentV2Config["selectionPolicy"],
                });
              }}
            >
              <option value="balanced">均衡</option>
              <option value="official_first">官方优先</option>
              <option value="third_party_first">第三方优先</option>
            </select>
          </label>
        </div>

        <h3 className="text-sm font-semibold">模型能力问卷</h3>
        <h3 className="text-sm font-semibold">最终字段</h3>
        <h3 className="text-sm font-semibold">TOML 预览</h3>

        <div className="grid gap-4 xl:grid-cols-2">
          {profileEntries.map(([profileKey, profile], profileIndex) => {
            const preview = previews[profileKey];
            const status = statusByProfileKey.get(profileKey);
            const overrides = profile.overrides ?? {};
            const nicknameValue =
              nicknameDrafts[profileKey] ??
              (
                overrides.nicknameCandidates ??
                preview?.nicknameCandidates ??
                []
              ).join(", ");
            const title =
              PROFILE_TITLES[profile.model] ?? `${profile.model} 子 Agent 能力`;
            return (
              <section
                key={profileKey}
                aria-label={title}
                className="space-y-3 rounded-lg border bg-background/80 p-4"
              >
                <div className="font-medium">{title}</div>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={profile.enabled}
                    onChange={(event) =>
                      updateProfile(profileKey, (current) => ({
                        ...current,
                        enabled: event.target.checked,
                      }))
                    }
                  />
                  启用此模型作为 V2 子 Agent
                </label>

                <fieldset className="grid gap-2" aria-label="任务优势">
                  <legend className="text-sm font-medium">任务优势</legend>
                  <div className="grid gap-1 sm:grid-cols-2">
                    {TASK_STRENGTHS.map((strength) => (
                      <label
                        key={strength.value}
                        className="flex items-center gap-2 text-xs"
                      >
                        <input
                          type="checkbox"
                          value={strength.value}
                          checked={profile.questionnaire.taskStrengths.includes(
                            strength.value,
                          )}
                          onChange={(event) => {
                            const selected =
                              profile.questionnaire.taskStrengths;
                            if (event.target.checked && selected.length >= 5) {
                              setStrengthLimitMessage("任务优势最多选择 5 项");
                              return;
                            }
                            setStrengthLimitMessage(null);
                            const taskStrengths = event.target.checked
                              ? selected.includes(strength.value)
                                ? selected
                                : [...selected, strength.value]
                              : selected.filter(
                                  (item) => item !== strength.value,
                                );
                            updateProfile(profileKey, (current) => ({
                              ...current,
                              questionnaire: {
                                ...current.questionnaire,
                                taskStrengths,
                              },
                            }));
                          }}
                        />
                        {strength.label}
                      </label>
                    ))}
                  </div>
                </fieldset>

                <div className="grid gap-2 sm:grid-cols-2">
                  <QuestionnaireSelect
                    label="优化目标"
                    value={profile.questionnaire.optimization}
                    options={[
                      ["speed", "速度"],
                      ["balanced", "均衡"],
                      ["quality", "质量"],
                    ]}
                    onChange={(value) =>
                      updateProfile(profileKey, (current) => ({
                        ...current,
                        questionnaire: {
                          ...current.questionnaire,
                          optimization:
                            value as typeof current.questionnaire.optimization,
                        },
                      }))
                    }
                  />
                  <QuestionnaireSelect
                    label="写入范围"
                    value={profile.questionnaire.writeScope}
                    options={[
                      ["read_only", "只读"],
                      ["bounded_changes", "有限修改"],
                      ["complex_changes", "复杂修改"],
                    ]}
                    onChange={(value) =>
                      updateProfile(profileKey, (current) => ({
                        ...current,
                        questionnaire: {
                          ...current.questionnaire,
                          writeScope:
                            value as typeof current.questionnaire.writeScope,
                        },
                      }))
                    }
                  />
                  <QuestionnaireSelect
                    label="模型偏好"
                    value={profile.questionnaire.preference}
                    options={[
                      ["preferred", "优先"],
                      ["eligible", "可用"],
                      ["fallback", "后备"],
                    ]}
                    onChange={(value) =>
                      updateProfile(profileKey, (current) => ({
                        ...current,
                        questionnaire: {
                          ...current.questionnaire,
                          preference:
                            value as typeof current.questionnaire.preference,
                        },
                      }))
                    }
                  />
                  <QuestionnaireSelect
                    label="推理强度"
                    value={profile.questionnaire.reasoningEffort}
                    options={[
                      ["auto", "自动"],
                      ["low", "低"],
                      ["medium", "中"],
                      ["high", "高"],
                      ["xhigh", "极高"],
                    ]}
                    onChange={(value) =>
                      updateProfile(profileKey, (current) => ({
                        ...current,
                        questionnaire: {
                          ...current.questionnaire,
                          reasoningEffort:
                            value as typeof current.questionnaire.reasoningEffort,
                        },
                      }))
                    }
                  />
                </div>

                <OverrideField
                  id={`codex-subagent-${profileIndex}-role-name`}
                  label="角色名称"
                  value={overrides.roleName ?? preview?.requestedRoleName ?? ""}
                  automatic={overrides.roleName === undefined}
                  restoreLabel="恢复角色名称自动值"
                  onChange={(value) =>
                    setOverride(profileKey, "roleName", value)
                  }
                  onRestore={() => restoreOverride(profileKey, "roleName")}
                />
                <OverrideField
                  id={`codex-subagent-${profileIndex}-description`}
                  label="角色描述"
                  value={overrides.description ?? preview?.description ?? ""}
                  automatic={overrides.description === undefined}
                  restoreLabel="恢复角色描述自动值"
                  multiline
                  onChange={(value) =>
                    setOverride(profileKey, "description", value)
                  }
                  onRestore={() => restoreOverride(profileKey, "description")}
                />
                <OverrideField
                  id={`codex-subagent-${profileIndex}-developer-instructions`}
                  label="开发者指令"
                  value={
                    overrides.developerInstructions ??
                    preview?.developerInstructions ??
                    ""
                  }
                  automatic={overrides.developerInstructions === undefined}
                  restoreLabel="恢复开发者指令自动值"
                  multiline
                  onChange={(value) =>
                    setOverride(profileKey, "developerInstructions", value)
                  }
                  onRestore={() =>
                    restoreOverride(profileKey, "developerInstructions")
                  }
                />
                <OverrideField
                  id={`codex-subagent-${profileIndex}-nickname-candidates`}
                  label="昵称候选"
                  value={nicknameValue}
                  automatic={overrides.nicknameCandidates === undefined}
                  restoreLabel="恢复昵称候选自动值"
                  onChange={(value) => {
                    setNicknameDrafts((current) => ({
                      ...current,
                      [profileKey]: value,
                    }));
                    setOverride(
                      profileKey,
                      "nicknameCandidates",
                      parseNicknames(value),
                    );
                  }}
                  onRestore={() =>
                    restoreOverride(profileKey, "nicknameCandidates")
                  }
                />
                <div className="grid gap-1 text-sm">
                  <span className="flex items-center justify-between gap-2">
                    <label
                      htmlFor={`codex-subagent-${profileIndex}-model-reasoning`}
                    >
                      模型推理强度
                    </label>
                    <span className="text-xs text-muted-foreground">
                      {overrides.modelReasoningEffort === undefined
                        ? "自动"
                        : "手工覆盖"}
                    </span>
                  </span>
                  <div className="flex gap-2">
                    <select
                      id={`codex-subagent-${profileIndex}-model-reasoning`}
                      className="min-w-0 flex-1 rounded-md border bg-background px-3 py-2"
                      value={
                        overrides.modelReasoningEffort ??
                        preview?.modelReasoningEffort ??
                        ""
                      }
                      onChange={(event) =>
                        setOverride(
                          profileKey,
                          "modelReasoningEffort",
                          event.target
                            .value as CodexSubagentExplicitReasoningEffort,
                        )
                      }
                    >
                      <option value="" disabled>
                        等待后端预览
                      </option>
                      <option value="low">低</option>
                      <option value="medium">中</option>
                      <option value="high">高</option>
                      <option value="xhigh">极高</option>
                    </select>
                    <Button
                      type="button"
                      variant="outline"
                      onClick={() =>
                        restoreOverride(profileKey, "modelReasoningEffort")
                      }
                    >
                      恢复模型推理强度自动值
                    </Button>
                  </div>
                </div>
                {previewErrors[profileKey] ? (
                  <p role="alert" className="text-xs text-rose-600">
                    {previewErrors[profileKey]}
                  </p>
                ) : null}
                <ProfileBackendOutput
                  profileKey={profileKey}
                  profile={profile}
                  preview={preview}
                  status={status}
                />
              </section>
            );
          })}
          {invalidProfileEntries.map(([profileKey]) => {
            const title =
              PROFILE_TITLES[profileKey] ?? `${profileKey} 子 Agent 能力`;
            return (
              <section
                key={profileKey}
                aria-label={title}
                className="space-y-3 rounded-lg border border-rose-300 bg-background/80 p-4"
              >
                <div className="font-medium">{title}</div>
                <p className="text-sm text-rose-700">
                  持久化的 profile 结构无效，原始条目尚未被修改。
                </p>
                <ProfileBackendOutput
                  profileKey={profileKey}
                  status={statusByProfileKey.get(profileKey)}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => repairProfile(profileKey)}
                >
                  使用默认问卷修复 {profileKey}
                </Button>
              </section>
            );
          })}
        </div>
      </fieldset>

      {strengthLimitMessage ? (
        <p role="alert" className="text-sm text-amber-700">
          {strengthLimitMessage}
        </p>
      ) : null}

      {statusError ? (
        <p role="alert" className="text-sm text-rose-600">
          {statusError}
        </p>
      ) : null}
      {statuses ? (
        <div className="space-y-2 rounded-lg border bg-background/80 p-4 text-sm">
          <p>生成来源：{statuses.generationSource}</p>
          {unassignedStatuses.map((status, index) => (
            <ProfileBackendOutput
              key={`${status.profileKey ?? "invalid"}-${index}`}
              profileKey={status.profileKey}
              status={status}
            />
          ))}
          {statuses.warnings.map((warning) => (
            <p key={warning} className="text-amber-700">
              {warning}
            </p>
          ))}
        </div>
      ) : null}

      <div className="flex items-center gap-3">
        <Button onClick={save} disabled={isSaving}>
          保存 V2 子 Agent 能力配置
        </Button>
        {saveMessage ? (
          <span aria-live="polite" className="text-sm text-emerald-700">
            {saveMessage}
          </span>
        ) : null}
        {saveError ? (
          <span role="alert" className="text-sm text-rose-600">
            {saveError}
          </span>
        ) : null}
      </div>
    </section>
  );
}

function ProfileBackendOutput({
  profileKey,
  profile,
  preview,
  status,
}: {
  profileKey?: string;
  profile?: CodexSubagentV2Profile;
  preview?: CodexSubagentProfilePreview;
  status?: CodexSubagentProfileStatus;
}) {
  if (!preview && !status) return null;
  const requestedRoleName =
    status?.requestedRoleName ?? preview?.requestedRoleName;
  const effectiveRoleName =
    status?.effectiveRoleName ?? preview?.effectiveRoleName;
  const warnings = Array.from(
    new Set([...(preview?.warnings ?? []), ...(status?.warnings ?? [])]),
  );

  return (
    <div
      role="region"
      aria-label={`${profileKey ?? "未识别 profile"} 后端预览状态`}
      className="space-y-2 rounded-lg border bg-muted/20 p-3 text-sm"
    >
      {status ? (
        <>
          <p>
            {profileKey
              ? `${profileKey}：${status.status}`
              : `生成状态：${status.status}`}
          </p>
          <p>生成状态：{status.status}</p>
          <p>
            Provider 类型：
            {status.providerKind ?? preview?.providerKind ?? "未知"}
          </p>
          <p>可路由：{status.routable ? "是" : "否"}</p>
          <p>
            已启用：
            {(status.enabled ?? profile?.enabled) ? "是" : "否"}
          </p>
          {status.fieldSources ? (
            <>
              <p>角色名称来源：{status.fieldSources.roleName}</p>
              <p>角色描述来源：{status.fieldSources.description}</p>
              <p>开发者指令来源：{status.fieldSources.developerInstructions}</p>
              <p>昵称候选来源：{status.fieldSources.nicknameCandidates}</p>
              <p>
                模型推理强度来源：
                {status.fieldSources.modelReasoningEffort}
              </p>
            </>
          ) : null}
          {status.roleFilePath ? <p>{status.roleFilePath}</p> : null}
          {status.model ? <p>模型：{status.model}</p> : null}
          {status.modelProvider ? (
            <p>模型 Provider：{status.modelProvider}</p>
          ) : null}
          {status.modelReasoningEffort ? (
            <p>推理强度：{status.modelReasoningEffort}</p>
          ) : null}
          {status.nonGenerationReason ? (
            <p>未生成原因：{status.nonGenerationReason}</p>
          ) : null}
        </>
      ) : null}

      {requestedRoleName || effectiveRoleName ? (
        <div className="grid gap-2 md:grid-cols-2">
          <div>
            <span className="font-medium">请求角色名</span>
            <div>{requestedRoleName ?? "后端未返回"}</div>
          </div>
          <div>
            <span className="font-medium">实际角色名</span>
            <div>{effectiveRoleName ?? "后端未返回"}</div>
          </div>
        </div>
      ) : null}

      {preview ? (
        <>
          <p>{preview.description}</p>
          <p>{preview.developerInstructions}</p>
          <div className="flex flex-wrap gap-2">
            {preview.nicknameCandidates.map((nickname) => (
              <span key={nickname}>{nickname}</span>
            ))}
          </div>
          <p>{preview.modelProvider}</p>
          <p>{preview.modelReasoningEffort}</p>
          <p>{preview.modelContextWindow}</p>
          <pre className="overflow-x-auto whitespace-pre-wrap rounded-lg border bg-slate-950 p-3 text-xs text-slate-100">
            {preview.tomlPreview}
          </pre>
        </>
      ) : (
        <p className="text-xs text-muted-foreground">后端预览不可用。</p>
      )}

      {warnings.map((warning) => (
        <p key={warning} className="text-amber-700">
          {warning}
        </p>
      ))}
    </div>
  );
}

function QuestionnaireSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<[string, string]>;
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid gap-1 text-sm">
      <span>{label}</span>
      <select
        className="rounded-md border bg-background px-3 py-2"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map(([optionValue, optionLabel]) => (
          <option key={optionValue} value={optionValue}>
            {optionLabel}
          </option>
        ))}
      </select>
    </label>
  );
}

function OverrideField({
  id,
  label,
  value,
  automatic,
  restoreLabel,
  multiline = false,
  onChange,
  onRestore,
}: {
  id: string;
  label: string;
  value: string;
  automatic: boolean;
  restoreLabel: string;
  multiline?: boolean;
  onChange: (value: string) => void;
  onRestore: () => void;
}) {
  const control = multiline ? (
    <textarea
      id={id}
      className="min-h-24 min-w-0 flex-1 rounded-md border bg-background px-3 py-2"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ) : (
    <input
      id={id}
      className="min-w-0 flex-1 rounded-md border bg-background px-3 py-2"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  );
  return (
    <div className="grid gap-1 text-sm">
      <span className="flex items-center justify-between gap-2">
        <label htmlFor={id}>{label}</label>
        <span className="text-xs text-muted-foreground">
          {automatic ? "自动" : "手工覆盖"}
        </span>
      </span>
      <div className="flex gap-2">
        {control}
        <Button type="button" variant="outline" onClick={onRestore}>
          {restoreLabel}
        </Button>
      </div>
    </div>
  );
}
