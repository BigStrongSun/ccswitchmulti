# Codex MultiRouter 子 Agent 独立工作区 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 MultiRouter 的 Sub-Agent 配置迁移到独立顶层工作区，修正 V1/V2 启用状态，并用可搜索、可筛选、单项折叠的 V2 profile 编辑器替代全部模型同时展开的长页面。

**Architecture:** `CodexRouterWorkspacePage.tsx` 继续拥有工作台导航、当前方案和协议切换；新增 `subagents` tab，只把现有 Sub-Agent 面板从 `RoutesTab` 迁移到独立页面，不改变保存协议。`CodexSubagentProfileEditor.tsx` 在现有 backend preview/status 和 focused persistence 上增加纯前端搜索、筛选、排序、Accordion、Collapsible 与 dirty feedback；后端仍是能力字段和 TOML 的唯一编译器。

**Tech Stack:** React 18、TypeScript、Tailwind CSS、Radix Tabs/Accordion/Collapsible、TanStack Query、Vitest、Testing Library、Tauri 2、Rust/Cargo、PowerShell/Pester。

## Global Constraints

- 当前源码基线为 `3.19.1-19`；最终四个版本源统一为 `3.19.1-20`。
- 不修改 `settingsConfig.codexRouting.subagentVersion` 或 `subagentV2` schema。
- 不修改 Rust compiler、managed role 文件格式、Provider 分类或模型目录同步契约。
- 不更改 Qwen 行为、reserved `spawn_agent` schema、父模型选择或代理路由。
- V1 `spawnAgentModels` 与 V2 profiles 在协议切换时必须同时保留。
- 继续使用 `update_codex_subagent_v2` 和携带完整 draft 的 backend reconcile；禁止恢复整份 Provider stale merge。
- 复用现有 `src/components/ui/accordion.tsx` 与 `collapsible.tsx`，不新增前端依赖。
- 所有本地提交正文最后一段必须是 `本次提交由BigStrongsSun完成`。
- 中间提交使用仓库 `.git/ccsm-empty-hooks` 作为临时空 `core.hooksPath`，不触发 post-commit release。
- 不 push、不创建 PR、不创建 GitHub Release。
- 构建安装只能使用独立隐藏事务进程；不得在普通交互 shell 中单独停止 CCSM。
- 现有未提交的 `docs/superpowers/plans/2026-08-10-codex-subagent-capability-injection.md` 与 `memory.md` 内容必须保留，除非某个任务明确只追加本轮内容。

---

### Task 1: RED — 独立顶层导航与协议状态契约

**Files:**
- Modify: `src/components/codex/CodexRouterWorkspacePage.test.ts:1510-1705`

**Interfaces:**
- Consumes: 现有 `CodexRouterWorkspacePage`、`WorkspaceTab`、`providersApi.update` mock、`createDraftRoutingPlan()` 测试 fixture。
- Produces: 对 `WorkspaceTab = "subagents"`、六项 tab 顺序、Sub-Agent 页面归属和 V1/V2 状态机的失败测试。

- [ ] **Step 1: 把旧“路由规则页直接出现 Sub-Agent”测试改为六项导航测试**

在原 `exposes V1 and V2 subagent settings...` 附近加入以下两个 helper：

```tsx
function createSubagentWorkspaceFixture() {
  const source: Provider = {
    id: "codex-deepseek",
    name: "DeepSeek",
    category: "custom",
    settingsConfig: {
      modelCatalog: {
        models: [
          { model: "deepseek-v4-flash" },
          { model: "deepseek-v4-pro" },
        ],
      },
    },
  };
  const draftPlan = createDraftRoutingPlan([source], [source]);
  const plan: Provider = {
    ...draftPlan,
    settingsConfig: {
      ...draftPlan.settingsConfig,
      modelCatalog: {
        ...draftPlan.settingsConfig?.modelCatalog,
        spawnAgentModels: ["deepseek-v4-pro", "deepseek-v4-flash"],
      },
      codexRouting: {
        ...draftPlan.settingsConfig?.codexRouting,
        subagentVersion: "v2",
        subagentV2: {
          schemaVersion: 1,
          selectionPolicy: "balanced",
          profiles: {
            "deepseek-v4-flash": {
              model: "deepseek-v4-flash",
              enabled: true,
              questionnaire: {
                taskStrengths: ["repository_exploration"],
                optimization: "speed",
                writeScope: "read_only",
                preference: "preferred",
                reasoningEffort: "medium",
              },
            },
          },
        },
      },
    },
  };
  return { source, plan };
}

function renderSubagentWorkspace(source: Provider, plan: Provider) {
  return renderWorkspace(
    React.createElement(CodexRouterWorkspacePage, {
      providers: [source, plan],
      isProxyRunning: true,
      isCodexTakeoverActive: true,
      activeProviderId: plan.id,
      initialProviderId: plan.id,
      initialTab: "routes",
      onEditProvider: vi.fn(),
      onDeletePlan: vi.fn(),
      onCreateProvider: vi.fn(),
    }),
  );
}
```

然后建立测试，先证明路由规则页不再承担 Sub-Agent 设置：

```tsx
it("moves Sub-Agent settings into a dedicated top-level workspace tab", async () => {
  const { source, plan } = createSubagentWorkspaceFixture();
  renderWorkspace(
    React.createElement(CodexRouterWorkspacePage, {
      providers: [source, plan],
      isProxyRunning: true,
      isCodexTakeoverActive: true,
      activeProviderId: plan.id,
      initialProviderId: plan.id,
      initialTab: "routes",
      onEditProvider: vi.fn(),
      onDeletePlan: vi.fn(),
      onCreateProvider: vi.fn(),
    }),
  );

  expect(
    within(screen.getByRole("tablist"))
      .getAllByRole("tab")
      .map((tab) => tab.textContent?.trim()),
  ).toEqual(["总览", "模型源", "路由规则", "子 Agent", "状态", "测试发布"]);
  expect(screen.queryByText("Sub-Agent 设置")).not.toBeInTheDocument();

  await userEvent.setup().click(screen.getByRole("tab", { name: "子 Agent" }));
  expect(await screen.findByText("Sub-Agent 设置")).toBeInTheDocument();
});
```

将 source/plan 建立过程抽成测试文件内的 `createSubagentWorkspaceFixture()`，固定返回 Flash、Pro 目录和 `spawnAgentModels: ["deepseek-v4-pro", "deepseek-v4-flash"]`，避免多个测试复制整段 Provider fixture。

- [ ] **Step 2: 增加当前 V2 和切换后 V1 的按钮语义测试**

```tsx
it("renders the active protocol as disabled and the inactive protocol as actionable", async () => {
  const { source, plan } = createSubagentWorkspaceFixture();
  renderSubagentWorkspace(source, plan);
  const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "子 Agent" }));

  const activeV2 = screen.getByRole("button", { name: "已启用 V2" });
  const inactiveV1 = screen.getByRole("button", { name: "启用 V1" });
  expect(activeV2).toBeDisabled();
  expect(activeV2).toHaveClass("bg-muted");
  expect(inactiveV1).toBeEnabled();
  expect(inactiveV1).toHaveClass("bg-blue-600");

  await user.click(inactiveV1);
  await waitFor(() => expect(providersApi.update).toHaveBeenCalledOnce());
  expect(screen.getByRole("button", { name: "已启用 V1" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "启用 V2" })).toBeEnabled();
  expect(screen.getByText(/重启 Codex\/app-server 并新建会话后生效/)).toBeInTheDocument();
});
```

保留并复用原断言，继续验证 `routes` 与 `spawnAgentModels` 原样写回；额外断言 V2 profile fixture 在 `providersApi.update` payload 内完全相等。

- [ ] **Step 3: 增加切换中防重复提交和失败保持旧状态测试**

让 `providersApi.update` 使用一个可控 Promise。点击 `启用 V1` 后，两张卡片的按钮都 disabled，目标按钮显示 `切换中…`；reject 后 `已启用 V2` 恢复，页面显示 `role="alert"` 的切换失败信息，且不显示 V1 配置。

- [ ] **Step 4: 运行 RED 测试并确认失败原因**

Run:

```powershell
pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts --no-file-parallelism -t "Sub-Agent|protocol"
```

Expected: FAIL，原因至少包含找不到 `tab` 名称“子 Agent”、找不到 `已启用 V2`，以及路由规则页仍能找到 `Sub-Agent 设置`；不得因 fixture、mock 或 TypeScript 语法错误失败。

- [ ] **Step 5: 提交 RED 契约**

```powershell
$hooks = Join-Path (git rev-parse --git-dir) 'ccsm-empty-hooks'
New-Item -ItemType Directory -Force -Path $hooks | Out-Null
git add -- src/components/codex/CodexRouterWorkspacePage.test.ts
git -c core.hooksPath=$hooks commit -m "test(subagent): 定义独立工作区与协议状态契约" -m "先以失败测试固定六项顶层导航、路由页移除 Sub-Agent、当前协议已启用状态、切换反馈和 V1/V2 数据保留。`n`n本次提交由BigStrongsSun完成"
```

---

### Task 2: GREEN — 迁移 Sub-Agent 页面并修正 V1/V2 状态机

**Files:**
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx:138-144`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx:3038-3185`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx:3566-3860`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx:4885-5565`
- Test: `src/components/codex/CodexRouterWorkspacePage.test.ts`

**Interfaces:**
- Consumes: Task 1 的 `subagents` tab 和状态契约；现有 `SpawnAgentCandidatesPanel` 的 V1 candidate 保存、V2 editor、live validation 和 `saveSubagentVersion()`。
- Produces: `WorkspaceTab` 新成员 `subagents`、`SubagentsTab`、独立协议页面和明确的 current/pending/error/success 状态。

- [ ] **Step 1: 扩展 WorkspaceTab 和六项响应式 TabsList**

将类型改为：

```ts
export type WorkspaceTab =
  | "overview"
  | "sources"
  | "routes"
  | "subagents"
  | "status"
  | "test";
```

把 TabsList 改为 `grid-cols-3 lg:grid-cols-6`，在 `routes` 与 `status` 之间加入：

```tsx
<WorkspaceTabTrigger value="subagents" icon={Bot} label="子 Agent" />
```

使用当前已引入的 Lucide 图标集合；若文件没有 `Bot`，在现有 `lucide-react` import 中加入 `Bot`，不添加依赖。

- [ ] **Step 2: 新增 SubagentsTab 并从 RoutesTab 删除面板**

在 `RoutesTab` 尾部保留 `ProviderModelRefreshPanel`，删除其中的 `SpawnAgentCandidatesPanel`。新增：

```tsx
function SubagentsTab({
  selectedPlan,
  selectedRoutes,
  onCreatePlan,
}: {
  selectedPlan: Provider | null;
  selectedRoutes: RouteEntry[];
  onCreatePlan: () => void;
}) {
  if (!selectedPlan) {
    return (
      <EmptyState
        icon={Bot}
        title="还没有可配置的 MultiRouter"
        detail="先创建或选择一个多路路由方案，再配置它的子 Agent 协议和模型能力。"
        actionLabel="创建多路路由"
        onAction={onCreatePlan}
      />
    );
  }
  return (
    <div className="space-y-3">
      <section aria-label="当前子 Agent 方案" className="rounded-lg border bg-card p-3">
        <div className="text-xs text-muted-foreground">当前 MultiRouter</div>
        <PlanCardContent provider={selectedPlan} compact />
      </section>
      <SpawnAgentCandidatesPanel
        selectedPlan={selectedPlan}
        selectedRoutes={selectedRoutes}
      />
    </div>
  );
}
```

在 `TabsContent value="subagents"` 传入当前 `selectedPlan` 和只属于该方案的 `selectedPlanRoutes`。不要从 `RoutesTab` 复制第二套选择方案状态。

- [ ] **Step 3: 为协议切换增加 pending 和 success 状态**

在 `SpawnAgentCandidatesPanel` 增加：

```ts
const [pendingSubagentVersion, setPendingSubagentVersion] =
  useState<CodexSubagentVersion | null>(null);
const [subagentVersionMessage, setSubagentVersionMessage] =
  useState<string | null>(null);
```

`saveSubagentVersion(version)` 在请求前设置 pending、清空 message/error；成功后才设置 active 和提示；finally 清空 pending。切换失败不修改 active：

```ts
setSubagentVersionMessage(
  `已启用 ${version.toUpperCase()}；重启 Codex/app-server 并新建会话后生效。`,
);
```

- [ ] **Step 4: 按状态机渲染协议按钮**

对每个版本计算 `isActive` 与 `isPending`。按钮必须使用：

```tsx
<Button
  size="sm"
  variant={isActive ? "outline" : "default"}
  disabled={isSavingSubagentVersion || isActive}
  className={cn(
    isActive
      ? "border-border bg-muted text-muted-foreground hover:bg-muted"
      : "bg-blue-600 text-white hover:bg-blue-500",
  )}
  onClick={() => saveSubagentVersion(version)}
>
  {isPending
    ? "切换中…"
    : isActive
      ? `已启用 ${version.toUpperCase()}`
      : `启用 ${version.toUpperCase()}`}
</Button>
```

将 success 放入 `aria-live="polite"`，error 保持 `role="alert"`。仅渲染当前协议配置：V1 为 direct override 面板，V2 为 `CodexSubagentProfileEditor`；非当前配置数据不删除。

- [ ] **Step 5: 运行工作台测试和类型检查**

```powershell
pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts --no-file-parallelism
pnpm typecheck
```

Expected: 工作台测试全部 PASS，typecheck exit 0；原模型源刷新、路由保存、状态页和测试发布测试不回归。

- [ ] **Step 6: 提交 GREEN 实现**

```powershell
git add -- src/components/codex/CodexRouterWorkspacePage.tsx src/components/codex/CodexRouterWorkspacePage.test.ts
git -c core.hooksPath=$hooks commit -m "feat(subagent): 增加独立配置工作区" -m "将 Sub-Agent 从路由规则页迁移到六项顶层导航中的独立页面，修正当前协议、待切换协议、切换中和失败状态，并保留 V1/V2 全量配置。`n`n本次提交由BigStrongsSun完成"
```

---

### Task 3: RED — V2 profile 搜索、筛选和渐进披露契约

**Files:**
- Modify: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx:640-740`
- Modify: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx:1350-1745`
- Modify: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx:1736-2584`

**Interfaces:**
- Consumes: 现有 `renderWorkspace()`、IPC fixtures、`flashRegion()`/`proRegion()`、preview/status mocks 和 reconcile payload assertions。
- Produces: 对 profile 默认排序、single Accordion、搜索、四种筛选、独立启用操作、两层渐进披露、新目录文案和粘性保存反馈的失败测试。

- [ ] **Step 1: 增加可复用的 profile 展开测试 helper**

在测试文件中加入：

```tsx
async function openProfile(user: ReturnType<typeof userEvent.setup>, model: string) {
  const trigger = await screen.findByRole("button", {
    name: new RegExp(`配置 ${model}`, "i"),
  });
  if (trigger.getAttribute("aria-expanded") !== "true") {
    await user.click(trigger);
  }
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  return screen.getByRole("region", { name: `${model} 子 Agent 配置` });
}

function flashRegion() {
  return screen.getByRole("region", {
    name: "deepseek-v4-flash 子 Agent 配置",
  });
}

function proRegion() {
  return screen.getByRole("region", {
    name: "deepseek-v4-pro 子 Agent 配置",
  });
}
```

保留 `flashRegion()` 供默认展开的 Flash 测试；把所有直接读取关闭 Pro 内容的测试改为先 `await openProfile(user, "deepseek-v4-pro")`。

- [ ] **Step 2: 增加默认排序和单项展开测试**

```tsx
it("sorts enabled routable profiles first and expands only one model", async () => {
  const user = userEvent.setup();
  await renderWorkspace();
  const triggers = await screen.findAllByRole("button", { name: /配置 deepseek-v4-/i });
  expect(triggers.map((button) => button.getAttribute("aria-expanded"))).toEqual([
    "true",
    "false",
  ]);
  expect(triggers[0]).toHaveTextContent("deepseek-v4-flash");
  await user.click(triggers[1]);
  expect(triggers[0]).toHaveAttribute("aria-expanded", "false");
  expect(triggers[1]).toHaveAttribute("aria-expanded", "true");
});
```

摘要断言必须覆盖 `第三方`、`可路由/不可路由`、`已启用`、`优先/后备`、最终 reasoning、任务强项和 `手工覆盖` 文本。

- [ ] **Step 3: 增加搜索和筛选测试**

建立四个独立测试：

- 搜索 `offline-writer` 只保留 Pro 摘要。
- 搜索 `third_party` 命中 Provider 类型。
- `已启用` 显示 enabled profiles。
- `不可路由` 只显示 status.routable=false；`待配置` 显示 enabled=false；`全部` 恢复完整列表。

筛选按钮用 `aria-pressed` 断言选择态。搜索无结果时断言“没有符合条件的子 Agent 模型”和“清除筛选”，点击后列表恢复。

- [ ] **Step 4: 增加高级字段和 TOML 默认折叠测试**

```tsx
it("keeps advanced overrides and generated TOML collapsed until requested", async () => {
  const user = userEvent.setup();
  await renderWorkspace();
  const flash = flashRegion();
  expect(within(flash).getByRole("group", { name: "任务优势" })).toBeVisible();
  expect(within(flash).queryByLabelText("角色描述")).not.toBeInTheDocument();
  expect(within(flash).queryByText(previewFixture.tomlPreview)).not.toBeInTheDocument();

  await user.click(within(flash).getByRole("button", { name: "高级字段" }));
  expect(within(flash).getByLabelText("角色描述")).toBeVisible();
  await user.click(
    within(flash).getByRole("button", { name: "生成结果与 TOML" }),
  );
  expect(within(flash).getByText(previewFixture.tomlPreview, {
    normalizer: getDefaultNormalizer({ trim: false, collapseWhitespace: false }),
  })).toBeVisible();
});
```

- [ ] **Step 5: 固定目录同步和 sticky dirty feedback 文案**

把旧按钮断言改为 `从模型目录添加可配置模型`，并断言解释文案完整出现。点击后仍必须调用：

```ts
expect(invoke).toHaveBeenCalledWith("reconcile_codex_subagent_v2_profiles", {
  providerId: "router",
  action: "sync_catalog",
  subagentV2: expectedUnsavedDraft,
});
```

修改问卷后断言保存条出现“有未保存更改”；保存成功后出现“配置已保存”和重启提示；focused `update_codex_subagent_v2` payload 保持原精确断言。

- [ ] **Step 6: 运行 RED 测试并确认结构性失败**

```powershell
pnpm exec vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx --no-file-parallelism -t "Accordion|搜索|筛选|高级字段|TOML|目录|未保存"
```

Expected: FAIL，原因是不存在搜索框/筛选按钮/Accordion trigger、新文案不存在、角色字段和 TOML 仍直接可见；IPC fixture 与既有保存测试不得先报错。

- [ ] **Step 7: 提交 RED 契约**

```powershell
git add -- src/components/codex/CodexSubagentV2ProfileEditor.test.tsx
git -c core.hooksPath=$hooks commit -m "test(subagent): 定义可折叠能力列表交互" -m "以失败测试固定已启用优先排序、单模型展开、搜索筛选、渐进披露、目录同步文案和长页面保存反馈，同时保留 focused persistence 精确 payload。`n`n本次提交由BigStrongsSun完成"
```

---

### Task 4: GREEN — 实现可搜索单项 Accordion profile 编辑器

**Files:**
- Modify: `src/components/codex/CodexSubagentProfileEditor.tsx:1-1207`
- Test: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`

**Interfaces:**
- Consumes: Task 3 UI contract；现有 `CodexSubagentV2Profile`、`CodexSubagentProfilePreview`、`CodexSubagentProfileStatus` 和全部 update/reconcile callbacks。
- Produces: `ProfileFilter`、排序/搜索派生列表、single controlled Accordion、嵌套 Collapsible、摘要状态和 sticky save bar；不导出新的持久化 API。

- [ ] **Step 1: 引入现有 UI primitives 和本地 view-state 类型**

加入 imports：

```ts
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Search } from "lucide-react";
```

定义：

```ts
type ProfileFilter = "enabled" | "draft" | "unroutable" | "all";

const PROFILE_FILTERS: Array<{ value: ProfileFilter; label: string }> = [
  { value: "enabled", label: "已启用" },
  { value: "draft", label: "待配置" },
  { value: "unroutable", label: "不可路由" },
  { value: "all", label: "全部" },
];
```

- [ ] **Step 2: 增加搜索、筛选、open key 和 dirty state**

在组件顶部 hooks 区声明：

```ts
const [profileSearch, setProfileSearch] = useState("");
const [profileFilter, setProfileFilter] = useState<ProfileFilter>("all");
const [openProfileKey, setOpenProfileKey] = useState("");
const isDirty = draft !== null && JSON.stringify(draft) !== persistedKey;
```

在现有 `if (!draft)` early return 之前增加稳定 key 计算和 effect，确保 hooks 顺序不随初始化状态改变：

```ts
const usableProfileEntries = useMemo(
  () =>
    draft
      ? Object.entries(readRawProfiles(draft)).filter(
          (entry): entry is [string, CodexSubagentV2Profile] =>
            isUsableProfile(entry[1]),
        )
      : [],
  [draft],
);
const usableProfileKeySignature = usableProfileEntries
  .map(([profileKey]) => profileKey)
  .join("\n");
const defaultOpenProfileKey =
  usableProfileEntries.find(([, profile]) => profile.enabled)?.[0] ??
  usableProfileEntries[0]?.[0] ??
  "";

useEffect(() => {
  const usableKeys = new Set(
    usableProfileKeySignature.split("\n").filter(Boolean),
  );
  setOpenProfileKey((current) =>
    usableKeys.has(current) ? current : defaultOpenProfileKey,
  );
}, [provider.id, usableProfileKeySignature, defaultOpenProfileKey]);
```

early return 后的 `profileEntries` 直接复用 `usableProfileEntries`。搜索和筛选不删除 open key，只影响当前可见列表。

- [ ] **Step 3: 实现稳定排序和匹配**

在 `statusByProfileKey` 建立后生成：

```ts
const visibleProfileEntries = [...profileEntries]
  .sort(([leftKey, left], [rightKey, right]) => {
    const leftStatus = statusByProfileKey.get(leftKey);
    const rightStatus = statusByProfileKey.get(rightKey);
    return (
      Number(right.enabled) - Number(left.enabled) ||
      Number(rightStatus?.routable ?? false) - Number(leftStatus?.routable ?? false) ||
      left.model.localeCompare(right.model, "en")
    );
  })
  .filter(([profileKey, profile]) => {
    const status = statusByProfileKey.get(profileKey);
    if (profileFilter === "enabled" && !profile.enabled) return false;
    if (profileFilter === "draft" && profile.enabled) return false;
    if (profileFilter === "unroutable" && status?.routable !== false) return false;
    const preview = previews[profileKey];
    const haystack = [
      profile.model,
      profileKey,
      preview?.requestedRoleName,
      preview?.effectiveRoleName,
      status?.requestedRoleName,
      status?.effectiveRoleName,
      status?.providerKind,
      preview?.providerKind,
    ].filter(Boolean).join(" ").toLocaleLowerCase();
    return haystack.includes(profileSearch.trim().toLocaleLowerCase());
  });
```

`draft` 表示 profile 已存在但未启用；不可路由与 enabled 可重叠，筛选各自独立。

- [ ] **Step 4: 重写 toolbar 文案和空结果**

在选择策略下放置可见 label `搜索子 Agent 模型`、`type="search"` 的 Input、四个 `aria-pressed` 筛选按钮，以及 `从模型目录添加可配置模型`。紧邻按钮显示：

```text
发现当前可路由模型并加入列表；新模型默认关闭，已有问卷和手工设置不会被覆盖。
```

`reconcile("sync_catalog", draft)` 成功 message 改为：

```text
已从模型目录添加可配置模型；已有设置保持不变
```

visible list 为空时渲染“没有符合条件的子 Agent 模型”和清除搜索/筛选的按钮。

- [ ] **Step 5: 把 profile 卡片改为 single Accordion**

使用：

```tsx
<Accordion
  type="single"
  collapsible
  value={openProfileKey}
  onValueChange={setOpenProfileKey}
  className="space-y-2"
>
  {visibleProfileEntries.map(([profileKey, profile], profileIndex) => (
    <AccordionItem key={profileKey} value={profileKey} className="rounded-lg border bg-background/80 px-4">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <AccordionTrigger
            aria-label={`配置 ${profile.model}`}
            className="py-3 hover:no-underline"
          >
            <ProfileSummary
              profile={profile}
              preview={previews[profileKey]}
              status={statusByProfileKey.get(profileKey)}
            />
          </AccordionTrigger>
        </div>
        <label className="flex shrink-0 items-center gap-2 text-xs">
          <Switch
            aria-label={`启用 ${profile.model} 作为 V2 子 Agent`}
            checked={profile.enabled}
            onCheckedChange={(checked) =>
              updateProfile(profileKey, (current) => ({
                ...current,
                enabled: checked,
              }))
            }
          />
          {profile.enabled ? "已启用" : "未启用"}
        </label>
      </div>
      <AccordionContent
        role="region"
        aria-label={`${profile.model} 子 Agent 配置`}
        className="space-y-4"
      >
        <fieldset className="grid gap-2" aria-label="任务优势">
          <legend className="text-sm font-medium">任务优势</legend>
          <div className="grid gap-1 sm:grid-cols-2">
            {TASK_STRENGTHS.map((strength) => (
              <label key={strength.value} className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  value={strength.value}
                  checked={profile.questionnaire.taskStrengths.includes(strength.value)}
                  onChange={(event) =>
                    updateTaskStrength(profileKey, strength.value, event.target.checked)
                  }
                />
                {strength.label}
              </label>
            ))}
          </div>
        </fieldset>
      </AccordionContent>
    </AccordionItem>
  ))}
</Accordion>
```

`updateTaskStrength` 定义为：

```ts
function updateTaskStrength(
  profileKey: string,
  strength: CodexSubagentTaskStrength,
  checked: boolean,
) {
  if (!draft) return;
  const profile = readRawProfiles(draft)[profileKey];
  if (!isUsableProfile(profile)) return;
  const selected = profile.questionnaire.taskStrengths;
  if (checked && selected.length >= 5) {
    setStrengthLimitMessage("任务优势最多选择 5 项");
    return;
  }
  setStrengthLimitMessage(null);
  const taskStrengths = checked
    ? selected.includes(strength)
      ? selected
      : [...selected, strength]
    : selected.filter((item) => item !== strength);
  updateProfile(profileKey, (current) => ({
    ...current,
    questionnaire: { ...current.questionnaire, taskStrengths },
  }));
}
```

紧接 fieldset 保留当前四个 `QuestionnaireSelect`，回调和枚举值完全不变；本 Task 的 Step 6 定义的两个 Collapsible 紧随其后。

上面的摘要行使用一个 `flex-1` wrapper 包住 `AccordionTrigger`，Switch 放在其相邻位置，不能嵌套进 trigger。`ProfileSummary` 作为同文件私有组件，只接收 profile、preview、status，使用以下签名和派生值：

```tsx
function ProfileSummary({
  profile,
  preview,
  status,
}: {
  profile: CodexSubagentV2Profile;
  preview?: CodexSubagentProfilePreview;
  status?: CodexSubagentProfileStatus;
}) {
  const preferenceLabels = {
    preferred: "优先",
    eligible: "可用",
    fallback: "后备",
  } as const;
  const providerKind = status?.providerKind ?? preview?.providerKind;
  const reasoning =
    status?.modelReasoningEffort ??
    preview?.modelReasoningEffort ??
    profile.questionnaire.reasoningEffort;
  const strengthLabels = profile.questionnaire.taskStrengths
    .slice(0, 3)
    .map((value) => TASK_STRENGTHS.find((item) => item.value === value)?.label)
    .filter(Boolean);
  const hasOverrides = Boolean(
    profile.overrides && Object.keys(profile.overrides).length > 0,
  );
  return (
    <div className="min-w-0 flex-1 text-left">
      <div className="truncate text-sm font-semibold">{profile.model}</div>
      <div className="mt-1 flex flex-wrap gap-1.5 text-xs">
        <Badge variant="outline">{providerKind === "official" ? "官方" : "第三方"}</Badge>
        <Badge variant="outline">{status?.routable === false ? "不可路由" : "可路由"}</Badge>
        <Badge variant="outline">{preferenceLabels[profile.questionnaire.preference]}</Badge>
        <Badge variant="outline">推理 {reasoning}</Badge>
        <Badge variant="outline">{hasOverrides ? "含手工覆盖" : "自动生成"}</Badge>
        {strengthLabels.map((label) => <Badge key={label} variant="outline">{label}</Badge>)}
      </div>
    </div>
  );
}
```

任务强项只使用 `TASK_STRENGTHS` 的中文 label，不复制 questionnaire 编译规则。

- [ ] **Step 6: 将手工字段和 backend output 放入独立 Collapsible**

问卷保持 AccordionContent 首屏可见。字段级 override 整体放入 `高级字段` Collapsible；`ProfileBackendOutput` 和 `previewErrors` 放入 `生成结果与 TOML` Collapsible。两个 Collapsible 默认 closed，并通过 `CollapsibleTrigger asChild` 使用 outline Button，保留 Radix 自动生成的 `aria-expanded`。

无效 profile 放在正常 Accordion 后面的独立“需要处理”区域，不参与正常 profile 搜索排序；现有修复、恢复和删除命令保持原 payload。

- [ ] **Step 7: 将保存区改为 sticky dirty bar**

替换普通底部 flex：

```tsx
<div className="sticky bottom-0 z-10 flex flex-wrap items-center gap-3 rounded-lg border bg-background/95 p-3 shadow-sm backdrop-blur">
  <span className="text-sm text-muted-foreground">
    {isDirty ? "有未保存更改" : "所有更改均已保存"}
  </span>
  <Button onClick={save} disabled={isSaving || !isDirty}>
    {isSaving ? "保存中…" : "保存 V2 子 Agent 配置"}
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
  {projectionWarning ? (
    <span role="status" aria-live="polite" className="text-sm text-amber-700">
      {projectionWarning}
    </span>
  ) : null}
</div>
```

保存成功 message 改为“配置已保存；重启 Codex/app-server 并新建会话后生效”。不要在保存后清空 search、filter 或 open key。

- [ ] **Step 8: 运行 editor focused tests 和 typecheck**

```powershell
pnpm exec vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx --no-file-parallelism
pnpm typecheck
```

Expected: editor 文件全部 PASS，typecheck exit 0；preview/status 请求数量、防抖、focused update、reconcile full draft、legacy initialization 和 alias restore 测试继续通过。

- [ ] **Step 9: 提交 GREEN 实现**

```powershell
git add -- src/components/codex/CodexSubagentProfileEditor.tsx src/components/codex/CodexSubagentV2ProfileEditor.test.tsx
git -c core.hooksPath=$hooks commit -m "feat(subagent): 重构可折叠能力配置列表" -m "增加模型搜索、状态筛选、已启用优先排序、单项 Accordion、字段与 TOML 渐进披露和粘性保存反馈，同时保持 backend compiler、focused update 与 reconcile 所有权。`n`n本次提交由BigStrongsSun完成"
```

---

### Task 5: 共享编辑器兼容与可访问性回归

**Files:**
- Modify: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx:1736-2584`
- Modify: `src/components/codex/CodexMultiRouterWizard.test.tsx:35-90`
- Modify: `src/components/codex/CodexRouterWorkspacePage.test.ts:1319-1705`

**Interfaces:**
- Consumes: Task 2/4 production UI；Wizard 与 Workspace 共用的 `CodexSubagentProfileEditor`；现有 save/refresh fixtures。
- Produces: 键盘语义、wizard 共享数据、保存刷新和旧配置兼容的完整回归证据。

- [ ] **Step 1: 更新旧四标题测试为新的层级契约**

删除要求“模型能力问卷/最终字段/TOML 预览同时作为 heading 出现”的旧测试，改为断言：选择策略 heading 可见、模型 summary trigger 存在、`高级字段` 和 `生成结果与 TOML` 初始 `aria-expanded=false`，展开后各自内容可访问。

- [ ] **Step 2: 增加键盘导航和相邻 Switch 测试**

聚焦第一个 Accordion trigger 后使用 ArrowDown，把焦点移动到第二个 trigger；Enter 展开第二个并关闭第一个。Tab 能从 trigger 到相邻模型 Switch，Space 只改变 enabled，不展开/关闭 Accordion。每个 trigger 都有 `aria-controls`，每个 Switch 都有包含 model id 的唯一 accessible name。

- [ ] **Step 3: 验证 Wizard 与 Workspace 使用同一份持久化配置**

在现有 `shares the wizard-saved V2 source with the remounted workspace` 测试中：先在 Wizard 展开 Flash、修改优化目标并保存；remount Workspace，进入“子 Agent” tab，展开 Flash，断言修改值保留。保存命令仍只能是 `update_codex_subagent_v2`，不得出现整 Provider `update_provider`。

- [ ] **Step 4: 验证目录临时消失、恢复和无效配置不受筛选破坏**

复用现有 backend-owned reconciliation fixtures：不可路由 profile 在“不可路由”筛选可见；catalog 恢复后回到“已启用”；invalid profile 始终在“需要处理”区域可见，搜索清空不会删除原始 invalid entry。

- [ ] **Step 5: 运行三组 focused tests**

```powershell
pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/components/codex/CodexMultiRouterWizard.test.tsx --no-file-parallelism
pnpm typecheck
pnpm format:check
git diff --check
```

Expected: 三个文件全部 PASS；typecheck、Prettier check、diff check exit 0。

- [ ] **Step 6: 提交兼容与可访问性结果**

```powershell
git add -- src/components/codex/CodexRouterWorkspacePage.test.ts src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/components/codex/CodexMultiRouterWizard.test.tsx
git -c core.hooksPath=$hooks commit -m "test(subagent): 收紧共享编辑器交互回归" -m "覆盖 Accordion 键盘行为、唯一启用控件、Wizard 到 Workspace 持久化、目录消失恢复和无效 profile 可见性，防止 UI 分层破坏既有能力注入链。`n`n本次提交由BigStrongsSun完成"
```

如果 Task 5 没有产生文件差异，不创建空提交；把上述命令输出记录到 Task 6 验证结果。

---

### Task 6: 全量验证与 `3.19.1-20` 版本提交

**Files:**
- Modify: `package.json:3`
- Modify: `src-tauri/Cargo.toml:3`
- Modify: `src-tauri/Cargo.lock:764`
- Modify: `src-tauri/tauri.conf.json:4`

**Interfaces:**
- Consumes: Task 1-5 已通过的前端实现和所有既有 Rust backend contracts。
- Produces: 四处一致的 `3.19.1-20` 源码版本、完整前后端绿灯和可追溯版本提交。

- [ ] **Step 1: 先运行完整源码门禁**

```powershell
pnpm exec vitest run --exclude '**/.worktrees/**' --no-file-parallelism
pnpm typecheck
pnpm format:check
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo check --manifest-path src-tauri/Cargo.toml --lib
cargo fmt --manifest-path src-tauri/Cargo.toml --check
git diff --check
```

Expected: Vitest、Rust lib tests 全部通过；只允许仓库既有明确 ignored tests；其余命令 exit 0。

- [ ] **Step 2: 用 apply_patch 把四个版本源改为 3.19.1-20**

只替换以下四处 exact value：

```text
package.json                         3.19.1-19 -> 3.19.1-20
src-tauri/Cargo.toml                 3.19.1-19 -> 3.19.1-20
src-tauri/Cargo.lock package cc-switch 3.19.1-19 -> 3.19.1-20
src-tauri/tauri.conf.json            3.19.1-19 -> 3.19.1-20
```

运行：

```powershell
$versions = @(
  (Get-Content package.json -Raw | ConvertFrom-Json).version,
  (Select-String -Path src-tauri/Cargo.toml -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value,
  (Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
)
if (@($versions | Where-Object { $_ -ne '3.19.1-20' }).Count -ne 0) { throw "Version sources diverged: $($versions -join ', ')" }
rg -n '3\.19\.1-19' package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
```

Expected: `$versions` 全为 `3.19.1-20`，最后 `rg` 无输出。

- [ ] **Step 3: 重跑 focused tests、typecheck、Cargo check 和 diff check**

```powershell
pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/components/codex/CodexMultiRouterWizard.test.tsx --no-file-parallelism
pnpm typecheck
cargo check --manifest-path src-tauri/Cargo.toml --lib
git diff --check
```

Expected: 全部 exit 0。

- [ ] **Step 4: 检查 staged scope 并提交版本**

只 stage 四个版本源；Task 1-5 已提交文件不应再次出现，两个预存 dirty 文档不得进入本提交：

```powershell
git add -- package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git diff --cached --name-only
git -c core.hooksPath=$hooks commit -m "chore(release): 升级子 Agent 工作区版本至 3.19.1-20" -m "统一 package、Cargo、lockfile 和 Tauri 版本源；源码态已通过完整前后端门禁，发布构建将在固定提交上单独运行一次。`n`n本次提交由BigStrongsSun完成"
```

Expected cached names exactly four files。

---

### Task 7: 固定提交构建、事务安装与真实 UI 验收

**Files:**
- Read: `scripts/local-release-pipeline.ps1`
- Read: `scripts/install-ccswitchmulti-transaction.ps1`
- Test: `scripts/tests/install-ccswitchmulti-transaction.Tests.ps1`
- Generated outside source tree: `C:\Users\sunda\Documents\LLMservice\cc-switch\.worktrees\最新版ccswitchmulti\`

**Interfaces:**
- Consumes: Task 6 固定 HEAD、`pnpm release:local`、事务脚本真实参数、安装态 PID 53924/端口 15721 的动态重新发现。
- Produces: 可安装 artifacts、metadata/签名/hash、事务成功结果、health 200 和实际界面截图验收。

- [ ] **Step 1: 锁定构建来源并运行一次 release pipeline**

```powershell
$releaseCommit = (git rev-parse HEAD).Trim()
$releaseVersion = (Get-Content package.json -Raw | ConvertFrom-Json).version
if ($releaseVersion -ne '3.19.1-20') { throw "Unexpected release version: $releaseVersion" }
pnpm release:local
if ($LASTEXITCODE -ne 0) { throw "release:local failed with exit code $LASTEXITCODE" }
```

不得并行启动第二个 pipeline。此前 release 目录中的 `3.19.1-20` 文件视为陈旧，只有本次 pipeline 完成后 metadata commit 等于 `$releaseCommit` 的产物可用。

- [ ] **Step 2: 验证 artifacts、metadata、签名、版本资源和 SHA-256**

```powershell
$releaseRoot = 'C:\Users\sunda\Documents\LLMservice\cc-switch\.worktrees\最新版ccswitchmulti'
$metadata = Get-Content (Join-Path $releaseRoot 'RELEASE-METADATA.md') -Raw
if ($metadata -notmatch [regex]::Escape("Commit: $releaseCommit") -or $metadata -notmatch 'Version: 3\.19\.1-20') {
  throw 'Release metadata does not bind artifacts to the fixed version commit'
}
$installerPath = Join-Path $releaseRoot 'windows\installer\CCSwitchMulti_3.19.1-20_x64-setup.exe'
$portablePath = Join-Path $releaseRoot 'windows\portable\CCSwitchMulti_3.19.1-20_x64-portable.zip'
$artifactExe = Join-Path $releaseRoot 'windows\raw-exe\CCSwitchMulti_3.19.1-20_x64.exe'
$signaturePath = "$installerPath.sig"
foreach ($path in @($installerPath, $portablePath, $artifactExe, $signaturePath)) {
  if (-not (Test-Path -LiteralPath $path)) { throw "Missing release artifact: $path" }
}
$artifactInfo = @($installerPath, $portablePath, $artifactExe) | ForEach-Object {
  [pscustomobject]@{ Path = $_; Length = (Get-Item $_).Length; Sha256 = (Get-FileHash $_ -Algorithm SHA256).Hash }
}
$rawVersion = (Get-Item $artifactExe).VersionInfo
if ($rawVersion.FileVersion -ne '3.19.1-20' -or $rawVersion.ProductVersion -ne '3.19.1-20') { throw 'Raw EXE version resources mismatch' }
if ((Get-Item $signaturePath).Length -le 0) { throw 'Updater signature is empty' }
```

- [ ] **Step 3: 运行 Pester 事务回归**

```powershell
$pesterResult = Invoke-Pester -Script '.\scripts\tests\install-ccswitchmulti-transaction.Tests.ps1' -PassThru
if ($pesterResult.FailedCount -ne 0) {
  throw "Transaction Pester suite failed: $($pesterResult.FailedCount) of $($pesterResult.TotalCount)"
}
```

Expected: 当前 Pester 3.4.0 下 `FailedCount=0`；不使用歧义的 `-Output Detailed`。

- [ ] **Step 4: 从当前运行态动态生成独立隐藏事务参数**

```powershell
$installedExecutable = 'C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe'
$installDirectory = Split-Path -Parent $installedExecutable
$uninstallExecutable = Join-Path $installDirectory 'uninstall.exe'
$listener = Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1
if (-not $listener) { throw 'No CCSM listener owns port 15721' }
$ccsmPid = [int]$listener.OwningProcess
$processPath = (Get-CimInstance Win32_Process -Filter "ProcessId=$ccsmPid").ExecutablePath
if (-not [string]::Equals($processPath, $installedExecutable, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Port owner path mismatch: $processPath"
}
$installerHash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
$installedHash = (Get-FileHash -LiteralPath $artifactExe -Algorithm SHA256).Hash
$currentHash = (Get-FileHash -LiteralPath $installedExecutable -Algorithm SHA256).Hash
$currentVersion = (Get-Item -LiteralPath $installedExecutable).VersionInfo.FileVersion
$backupRoot = 'C:\Users\sunda\AppData\Local\CCSwitchMultiTransactionBackups\ccsm-3.19.1-20-subagent-ui'
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null
$transactionLog = Join-Path $backupRoot 'transaction-result.json'
$transactionError = Join-Path $backupRoot 'transaction-result.stderr.txt'
$transactionArgs = @(
  '-NoProfile','-ExecutionPolicy','Bypass','-File',(Resolve-Path '.\scripts\install-ccswitchmulti-transaction.ps1'),
  '-InstallerPath',$installerPath,'-ExpectedInstallerHash',$installerHash,
  '-ExpectedCurrentVersion',$currentVersion,'-ExpectedCurrentHash',$currentHash,
  '-ExpectedInstalledVersion','3.19.1-20','-ExpectedInstalledHash',$installedHash,
  '-CurrentPid',$ccsmPid,'-InstalledExecutable',$installedExecutable,'-InstallDirectory',$installDirectory,
  '-UninstallExecutable',$uninstallExecutable,'-ConfigPath','C:\Users\sunda\.cc-switch',
  '-RegistryKey','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti',
  '-Port','15721','-HealthUri','http://127.0.0.1:15721/health','-TimeoutSeconds','90','-BackupRoot',$backupRoot
)
```

- [ ] **Step 5: 启动一个事务进程并只等待该 PID**

```powershell
$transaction = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList $transactionArgs -RedirectStandardOutput $transactionLog -RedirectStandardError $transactionError
$transaction.WaitForExit()
if (-not (Test-Path -LiteralPath $transactionLog)) { throw 'Transaction did not create result JSON' }
try {
  $result = Get-Content -Raw $transactionLog | ConvertFrom-Json -ErrorAction Stop
} catch {
  throw "Transaction result JSON is invalid: $($_.Exception.Message)"
}
if ($result.Status -ne 'Success' -or $transaction.ExitCode -ne 0 -or $result.Error -or $result.RollbackError) {
  throw "Transaction did not install successfully: $($result | ConvertTo-Json -Compress)"
}
```

事务内部必须完成 stop、port wait、uninstall、install、hidden relaunch、health/version/hash 和失败回滚；外部 shell 不运行 Stop-Process。`WaitForExit()` 只针对事务 PowerShell PID，不使用 `Start-Process -Wait` 等待长期存活的 CCSM 后代。

- [ ] **Step 6: 事务外复核运行态**

```powershell
$newListener = Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1
if (-not $newListener) { throw 'Installed CCSM is not listening on 15721' }
$health = Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:15721/health'
if ($health.StatusCode -ne 200) { throw "CCSM health failed: $($health.StatusCode)" }
$installedVersion = (Get-Item $installedExecutable).VersionInfo
if ($installedVersion.FileVersion -ne '3.19.1-20' -or $installedVersion.ProductVersion -ne '3.19.1-20') { throw 'Installed version resources mismatch' }
if ((Get-FileHash $installedExecutable -Algorithm SHA256).Hash -ne $installedHash) { throw 'Installed EXE hash differs from release raw EXE' }
```

- [ ] **Step 7: 用实际安装界面验证核心流程并保存截图**

打开 CCSM → Codex 多模型路由，检查并记录：

1. 顶部顺序为“总览、模型源、路由规则、子 Agent、状态、测试发布”。
2. 路由规则页不再出现 Sub-Agent 设置。
3. 子 Agent 页当前 V2 显示灰色 disabled `已启用 V2`，V1 显示蓝色 `启用 V1`。
4. Flash/Pro 摘要位于前部；搜索 `deepseek-v4-pro` 可定位 Pro；不可路由筛选只显示对应模型。
5. 打开 Pro 后 Flash 自动折叠；高级字段和 TOML 初始关闭，点击后才显示。
6. “从模型目录添加可配置模型”旁边有完整安全说明。
7. 修改一个无副作用字段后出现 sticky“有未保存更改”，恢复自动值或保存后状态正确。

截图保存到 `C:\Users\sunda\Documents\LLMservice\cc-switch\.worktrees\subagent-v2-capability-injection\artifacts\ui-acceptance\3.19.1-20-subagent-workspace.png`；截图只用于验收，不加入 Git。

---

### Task 8: 项目 memory 与交付证据提交

**Files:**
- Modify: `memory.md`
- Read: `docs/superpowers/plans/2026-08-10-codex-subagent-capability-injection.md`
- Read: `docs/superpowers/specs/2026-08-11-codex-subagent-workspace-ui-design.md`

**Interfaces:**
- Consumes: Task 6 全量测试数字、Task 7 commit/version/hash/transaction/PID/health/UI screenshot 实证。
- Produces: 后续工作可直接复用的项目知识和最终本地 closeout commit。

- [ ] **Step 1: 在 memory.md 顶部追加 2026-08-11 UI closeout**

新增一节，必须记录：

- 根因：Sub-Agent 由 `RoutesTab` 页尾直接渲染；active 按钮文字与 disabled 仅取决于 saving；profiles 全量展开并内联 backend output/TOML。
- 最终结构：六项 tab、独立 `subagents` 页面、协议状态机、search/filter/single Accordion/Collapsible/sticky save。
- 持久化边界：V1/V2 数据保留、focused update、full-draft reconcile、backend compiler 未改。
- 精确测试通过数字与命令。
- `3.19.1-20` 固定 commit、installer/raw/portable SHA-256、transaction id、新 PID、health 200 和截图路径。
- 本轮 Codex Web/Matrix 交叉验证结果：Radix 官方结论一致，Matrix 访问 W3C 被 403 challenge 阻挡。
- 未 push、未 PR、未 GitHub Release。

- [ ] **Step 2: 只 stage 本轮 memory hunk**

先运行：

```powershell
git diff -- memory.md
git add -p -- memory.md
git diff --cached -- memory.md
```

在交互式 staging 中只选择新追加的 `2026-08-11 Codex Sub-Agent 独立工作区 3.19.1-20` hunk；原先已存在但未提交的 memory hunks 保持 unstaged。若新节与旧 dirty hunk 连成一个不可分割 hunk，使用 `s` 拆分；仍不可分割时使用 `e`，保留 staged patch 中仅本轮新增行。

- [ ] **Step 3: 提交交付证据**

```powershell
git -c core.hooksPath=$hooks commit -m "docs(subagent): 记录 3.19.1-20 UI 交付证据" -m "记录独立子 Agent 工作区的根因、交互边界、完整测试、构建哈希、事务安装和真实界面验收，保留原有未提交 memory 内容不进入本次提交。`n`n本次提交由BigStrongsSun完成"
```

- [ ] **Step 4: 最终检查**

```powershell
git status --short
git log -8 --oneline
git diff --check
```

Expected: 只剩进入本轮前就存在的 `docs/superpowers/plans/2026-08-10-codex-subagent-capability-injection.md` 与 `memory.md` 未提交内容；实现、版本和本轮 memory hunk 均已有本地提交。CCSM `3.19.1-20` 正在运行，15721 health HTTP 200。

## Plan Self-Review

- Spec coverage: Task 1-2 覆盖导航与协议状态；Task 3-5 覆盖模型定位、折叠、文案、保存反馈、错误和可访问性；Task 6-8 覆盖版本、完整验证、事务安装、真实 UI 与 memory。
- Type consistency: 全文统一使用 `WorkspaceTab = "subagents"`、`ProfileFilter = "enabled" | "draft" | "unroutable" | "all"`、`openProfileKey`、`pendingSubagentVersion` 和现有 `CodexSubagentVersion`。
- Persistence consistency: 所有 V2 保存继续使用 `update_codex_subagent_v2`；目录同步继续使用 `reconcile_codex_subagent_v2_profiles` 的 `sync_catalog` action 和完整 `subagentV2` draft。
- Scope consistency: 不包含 backend schema/compiler、Qwen、spawn schema、代理转发、PR 或公开 release 变更。
