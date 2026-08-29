# Codex 混合协议 Provider Set 原子拆分设计

## 目标

Codex 普通模式必须对每个启用模型真实探测 Chat Completions 和 Responses，并为该模型选择最适合真实 Codex 工具事务的唯一协议。保存结果按模型最终选择归一化：

- 全部模型选择同一协议：保存一个单协议普通 Provider；
- 一部分模型选择 Responses、另一部分选择 Chat：后端原子生成两个单协议叶子 Provider，并以原 Provider ID 保存一个稳定的 schema-v2 MultiRouter 门面；
- 任一启用模型只有 `Partial`、`Failed`、`Unavailable` 或无唯一选择：普通模式阻止保存，不得把它猜测归组；
- 请求级逐模型动态协议只允许服务于尚未迁移的旧混合数据，不能成为新建、编辑或向导保存的新架构。

本设计同时覆盖 Provider 新建/编辑、模型目录、顶层协议字段、Codex TOML、协议探测档案、依赖 MultiRouter、Universal Provider、活动状态、配置向导和前端确认交互。

## 非目标

- 不把“端点能同时返回 Chat/Responses 200”作为拆分依据；
- 不按模型名称、厂商、URL 或历史 `apiFormat` 猜协议；
- 不允许一个新 Provider 通过 `modelCatalog.models[].apiFormat` 长期动态切换协议；
- 不在本设计中修改推理强度档位、Subagent 模型选择或模型顺序算法；这些入口可由向导展示，但语义保持独立；
- 不把 raw reasoning 改标成 summary，也不让 reasoning 形态覆盖更完整的工具事务能力。

## 术语与身份

### 逻辑源 Provider

用户在 Provider 设置页配置的模型源。其稳定 ID 记为 `source_id`。用户始终编辑这个逻辑源，而不是直接编辑自动生成的叶子。

### 单协议 Provider

顶层 `meta.apiFormat`、`settingsConfig.apiFormat`、TOML `wire_api`、模型目录和档案全部指向同一上游协议的普通 Provider。

### Provider Set

一个逻辑源的持久化形态：

```text
统一协议：source_id（普通 Provider）

混合协议：source_id（schema-v2 MultiRouter 门面）
├─ source_id--ccsm-responses（普通 Responses 叶子）
└─ source_id--ccsm-chat（普通 Chat 叶子）
```

叶子 ID 必须确定性生成。若目标 ID 已被普通用户 Provider 占用，prepare 返回冲突，不能覆盖。只有带有匹配 `codexProtocolSet` 生成标记、父 ID 和 transport 的既有叶子才可更新或删除。

### 门面

门面是 schema-v2 MultiRouter，不是第三种上游协议。Codex 对 CCSM 的外层协议始终是 Responses；Chat 仅表示 CCSM 到真实上游的内部转换。

原 Provider ID 保留给门面，使以下身份稳定：

- 当前活动 Provider；
- Codex 接管和模型目录投影；
- 历史会话所记录的 Provider 身份；
- UI 中的逻辑模型源；
- 其他 MultiRouter 对原 Provider 的引用。

## 探测与选择规则

### 探测输入

每个启用模型都必须经过当前 production-equivalent request preparer，分别探测：

- Responses；
- Chat Completions；
- baseline JSON；
- baseline SSE；
- Codex 工具 Schema；
- 强制工具调用；
- 携带真实工具结果的续轮。

receipt 必须绑定最终 effective Provider/model、request-policy fingerprint、脱敏凭据指纹、probe corpus/budget/preparer version、有效期和逻辑源 revision。

### 唯一选择

selector 对每个模型输出唯一 `selected_transport`：

1. 先比较完整的 Codex 事务能力；
2. 能力相同时比较 Codex 可呈现推理质量；
3. 仍相同时优先原生 Responses。

一个模型即使两个协议均 Verified，也只能进入 selector 选中的一个分组，绝不能复制到两个叶子。

### 阻断条件

普通模式下，任一启用模型满足以下条件即返回 `Blocked`，且 commit 必须零写入：

- 没有 probe record；
- record 过期或 fingerprint/revision/target 不匹配；
- readiness 不是 `Verified`；
- 没有唯一 `selected_transport`；
- 同一个模型出现互相冲突的有效选择；
- 选中协议与 record target 不一致。

禁用模型不参与分组，也不得借此删除它原有的权威目录配置；它仍保存在逻辑源目录中，重新启用后必须重新探测。

## Provider Set 规划模型

后端新增一个纯规划层，建议文件为：

`src-tauri/src/codex_multirouter/provider_set.rs`

核心类型：

```rust
pub struct CodexProviderSetPreview {
    pub digest: String,
    pub source_provider_id: String,
    pub plan: CodexProviderSetPlan,
    pub responses_models: Vec<String>,
    pub chat_models: Vec<String>,
    pub blocked_models: Vec<CodexProviderSetBlockedModel>,
}

pub enum CodexProviderSetPlan {
    Single { transport: TransportKind },
    Split {
        responses_provider_id: String,
        chat_provider_id: String,
    },
    Router,
    Blocked,
}

pub struct PreparedCodexProviderSetMutation {
    pub digest: String,
    pub source_before: Option<Provider>,
    pub source_after: Provider,
    pub upsert_providers: Vec<Provider>,
    pub delete_provider_ids: Vec<String>,
    pub protocol_profiles: Vec<ProtocolCompatibilityRecord>,
    pub dependent_router_updates: Vec<Provider>,
    pub active_provider_after: Option<String>,
    pub universal_update: Option<PreparedUniversalProviderMutation>,
}
```

`prepare` 是纯函数或只读数据库操作；不得写 Provider、profile、Universal、活动状态或文件。`commit` 只接受 prepare 产生的 digest/intent，并在 transaction 中重新验证依赖 revision 与 ID 所有权。

### Plan::Single

- 使用原 `source_id` 保存普通 Provider；
- 清除所有模型级 `apiFormat/api_format/wire_api`；
- 同步写入唯一顶层协议：
  - `meta.apiFormat`；
  - `settingsConfig.apiFormat`；
  - TOML `model_providers.<id>.wire_api`；
- 保留完整逻辑源模型目录，不因协议选择删除模型；
- 若旧状态是 split，删除两个受管叶子并把依赖 Router 的叶子引用折叠回 `source_id`；
- 原活动 Provider 若为门面或任一受管叶子，统一切回 `source_id`。

### Plan::Split

- `source_id` 保存 schema-v2 MultiRouter 门面；
- Responses 叶子只包含 selector 选择 Responses 的启用模型；
- Chat 叶子只包含 selector 选择 Chat 的启用模型；
- 每个叶子顶层协议、TOML、目录模型和 profile 完全同质；
- 门面 routes 只引用两个叶子，不允许 Router→Router；
- 门面按 canonical/alias 模型精确路由到对应叶子；
- 门面保存逻辑源权威配置和完整模型目录的恢复材料，但运行目录仍由 MultiRouter compiler 从叶子实时投影；
- 原逻辑源若当前活动，commit 后仍保持 `source_id` 活动；
- 不自动激活任一叶子。

### Plan::Router

当用户显式编辑一个普通 MultiRouter 时，不把它当作逻辑源拆分。其 route 下的最终 Provider/model 仍各自深测，但 Router 保存必须消费每个最终 target 的有效 receipt，并只引用已经规范化的普通叶子或普通单协议 Provider。

Router 引用一个 split 门面时，prepare 必须在同一层将其原子展开为两个叶子并保留原规则的 match、alias、enabled 和 fallback 语义，禁止持久化 Router→Router。

### Plan::Blocked

返回结构化 blocked 模型、失败阶段、分类和重试目标。不得修改前端草稿、Provider、profile、活动状态、Universal 或任何 live 文件。

## 持久化元数据

在 `settingsConfig.codexProtocolSet` 保存版本化、非秘密的恢复元数据。

门面示例：

```json
{
  "version": 1,
  "role": "facade",
  "responsesProviderId": "source--ccsm-responses",
  "chatProviderId": "source--ccsm-chat",
  "sourceModelCatalog": { "models": [] }
}
```

叶子示例：

```json
{
  "version": 1,
  "role": "leaf",
  "parentProviderId": "source",
  "transport": "open_ai_responses"
}
```

`sourceModelCatalog` 是编辑恢复材料，不是新的长期运行 SSOT。保存时必须由逻辑源草稿重新生成；MultiRouter compiler 和 `/v1/models` 仍从叶子 Provider 当前目录派生。

门面可以保留逻辑源的认证、公共请求配置、网站、分类和用户可编辑元数据，便于再次探测与重建叶子。认证材料不得复制进 `codexProtocolSet`。

## Profile 重绑定

profile 不能仍指向门面或旧逻辑源 target：

- Single：record 绑定 `source_id + model + selected_transport`；
- Split：Responses record 绑定 Responses 叶子，Chat record 绑定 Chat 叶子；
- 逻辑 route/public model 身份保留在 record target 中；
- 旧 profile 在新的 request-policy fingerprint 或 preparer version 下失效；
- Partial/Failed record 可作为诊断证据保存到独立 probe history，但不能写入可执行 compatibility profile 集合。

## 原子事务

Provider Set commit 使用一个 SQLite transaction 完成：

1. 重新读取 source、受管叶子、依赖 Router、Universal definition 和活动状态；
2. 验证 prepare digest、revision、receipt、ID 所有权和依赖图未变化；
3. upsert source/门面和叶子；
4. upsert 仅 Verified 的 profile；
5. 更新所有依赖 MultiRouter routes、`defaultRouteId`、alias/selection 引用；
6. 更新 Universal definition 和 Claude/Codex/Gemini 子 Provider；
7. 更新 DB `is_current`，保证一个应用只有一个 current Provider；
8. 删除已不需要且带正确生成标记的叶子/profile；
9. 提交 transaction。

任一步失败必须整体回滚。不得先写 Universal definition，再逐个保存子 Provider；不得从前端循环调用两次 `onSubmit`。

Codex live config、模型缓存和设置文件是 DB commit 后的可重试派生投影：

- 投影失败不回滚已经提交的数据库事务；
- mutation 返回 `committed_with_projection_error` 和可重试诊断；
- 下一次启动、激活或显式修复可从 DB 权威状态重建；
- 不能出现数据库只保存一个叶子但 UI 提示拆分成功。

## 依赖 MultiRouter 迁移

若既有 Router route 的 `targetProviderId == source_id`：

- Single 保存后引用保持 `source_id`；
- Split 保存后按原 route 的模型选择计算两个新 route；
- 每个新 route 只保留属于对应叶子的 canonical 模型和 alias；
- `mode=all` 展开成两个 `mode=all` 叶子 route；
- `mode=include` 按模型分割 include 集合；空分支不生成 route；
- route ID 确定性派生，`defaultRouteId` 指向原默认 route 所覆盖模型的首个有效叶子；若无法唯一确定，prepare 阻断并返回可操作错误；
- route 优先级、enabled、fallback 和用户标签保持；生成标签可追加协议后缀，但不能改变用户可见源名称。

删除或折叠时只处理带生成标记的 route/leaf，不能误删用户手工 route。

## Universal Provider

Universal 保存必须先 prepare 全部应用子 Provider：

- Codex 子 Provider 使用本设计的 Single/Split 计划；
- Claude/Gemini 子 Provider 保持各自既有转换；
- definition、全部子 Provider、profiles、依赖 Router 和 current 状态一次事务提交；
- 任一 Codex 模型 blocked、任一子 Provider 校验失败或 ID 冲突，整体零写入；
- Universal 来源 ID/父子关系稳定，自动叶子记录其 Codex 逻辑父 ID，不能伪装成新的 Universal source。

## Provider 保存 API

新增结构化 prepare/commit 边界，而不是继续让前端把选择结果直接写进 Provider：

```text
prepare_codex_provider_set(providerDraft, probeReceiptIds)
  -> CodexProviderSetPreview

commit_codex_provider_set(providerDraft, commitIntent, digest)
  -> CodexProviderSetCommitOutcome
```

`commitIntent` 只能表达：

- 接受 `Single`；
- 确认执行 `Split`；
- 高级模式按整个 Provider 选择单一协议并确认风险。

前端不能提交每个模型的协议映射，也不能伪造叶子 Provider。digest 覆盖草稿的逻辑配置、启用模型、receipt 身份和计划结果；用户修改模型、Key、URL、headers/body override、TOML、协议高级设置或目录后，旧确认立即失效。

新增 Provider 应在打开表单或首次 prepare 时获得稳定 UUID。若探测阶段使用 draft ID，receipt lease key 必须按物理 target/fingerprint 重绑定，并在 commit 时再次校验；不能让临时 ID 成为持久化身份。

## 普通模式交互

### 保存前置

点击保存时：

1. 后端规范化草稿并计算 readiness identity；
2. 缺少当前有效深测结果时，打开真实深测进度，不保存；
3. 深测完成后自动调用 prepare；
4. `Single` 直接提交；
5. `Split` 显示拆分确认；
6. `Blocked` 显示失败模型、阶段、分类和重试入口，保存保持禁用。

### Split 确认弹窗

普通用户不选择协议，只确认系统基于实测结果产生的计划。弹窗显示：

- 说明：同一模型源内，不同模型通过了不同的最佳 Codex 协议；
- Responses 组及模型列表；
- Chat 组及模型列表；
- 保存后仍作为一个模型源使用，系统会自动路由；
- 操作：`返回调整模型`、`确认按协议拆分`。

不得显示让普通用户选择 raw/summary 字段、逐模型协议或两个 Provider 名称的控件。

### 编辑自动拆分门面

Provider 列表只显示一个逻辑源卡片，可用辅助标签说明“自动路由：Responses N / Chat M”。打开编辑时：

- 后端把门面+叶子恢复为逻辑源草稿；
- 用户编辑完整权威模型目录；
- 重新保存必须重新验证受影响模型；
- 用户不直接编辑派生叶子；
- 删除逻辑源时后端原子删除门面、受管叶子、profiles，并更新依赖 Router/Universal；删除仍遵循既有危险操作确认。

## 高级模式

高级模式只允许整个逻辑 Provider 手动选择一个上游协议，不允许逐模型混合：

- Chat 或 Responses 二选一；
- 明确提示跳过真实探测可能导致 400/422、工具续轮失败、推理不可见或提前结束；
- 保存为 Single；
- 原 split Provider 改为高级单协议时执行 split→uniform 折叠；
- reasoning projection、tool schema、history replay 仍是独立高级设置，但不能改变 Provider Set 分组规则。

## 配置向导同步

`CodexMultiRouterWizard` 不再拥有独立浅探测、协议推断或逐 Provider `providersApi.update` 循环。

向导流程：

1. 选择或新建 Provider 模型源；
2. 在 Provider 配置组件中同步模型、选择启用模型；
3. 复用正式深探测进度与 receipt；
4. 对每个逻辑源调用 Provider Set prepare；
5. Single 自动通过，Split 展示与 Provider 设置页相同的分组确认，Blocked 停在该模型源并提供重试；
6. 后端批量 prepare 最终 Router plan，展开 split 门面为叶子，验证无 Router→Router；
7. 一个 transaction 保存所有 Provider Set、最终 MultiRouter、profiles、Universal/current 状态；
8. 成功页保留历史修复入口，并提供 Subagent、推理强度和模型顺序的后续入口；这些设置不并入协议事务。

Provider 设置页和向导必须复用同一后端 prepare/commit API、同一进度组件和同一 Split/Blocked 展示组件，不能复制选择算法。

## 旧混合数据迁移

启动或读取 Provider 时识别以下旧形态：

- 顶层协议与 `modelCatalog.models[].apiFormat` 不一致；
- 同一普通 Provider 的模型级协议存在多个值；
- profile 对不同模型选择不同 transport，但 Provider 不是受管 split 门面。

处理方式：

- 运行时在存在未过期、fingerprint 匹配的 Verified profile 时，允许按请求模型选择 transport；
- 缺少 profile 时使用顶层协议并记录迁移诊断，不能按模型名猜测；
- Provider UI 显示“旧版混合协议配置，需重新验证并迁移”；
- 重新保存必须走 Provider Set prepare/commit；
- 迁移成功后清除所有模型级协议；
- 新保存路径和向导不得产生旧混合数据。

## 错误语义

后端返回稳定错误码和结构化字段：

- `codex_provider_set_probe_required`；
- `codex_provider_set_probe_stale`；
- `codex_provider_set_model_blocked`；
- `codex_provider_set_leaf_id_conflict`；
- `codex_provider_set_dependency_changed`；
- `codex_provider_set_router_expansion_ambiguous`；
- `codex_provider_set_manual_mixed_protocol_forbidden`；
- `codex_provider_set_projection_pending`。

错误消息不得包含 Key、Authorization、Cookie、完整响应正文或用户 prompt。

## TDD 验收矩阵

### 纯规划

- 同一模型双协议通过，但只进入 selector 选中的一组；
- 全 Responses 生成 Single；
- 全 Chat 生成 Single；
- mixed Verified 生成门面和两个同质叶子；
- Partial/Failed/无 record 生成 Blocked；
- disabled 模型不参与分组；
- leaf ID 冲突拒绝；
- 门面编辑恢复完整逻辑源目录。

### 原子持久化

- 第二叶子写入失败，source/第一叶子/profile/current 全部回滚；
- profile 重绑定到正确叶子；
- split 时活动 source ID 保持；
- split→uniform 删除受管叶子并保留 source ID；
- 依赖 Router 的 include/all/alias/default 原子展开与折叠；
- ambiguous default 阻断且零写入；
- Universal 任一子 Provider 失败整体回滚；
- DB 提交后 live projection 失败返回可重试状态而不破坏 DB 权威状态。

### 保存与 UI

- 普通模式无当前 probe 时保存只启动探测；
- Single 不弹确认直接保存；
- Split 显示两组模型且只有返回/确认；
- 修改草稿后旧 digest/确认失效；
- Blocked 显示失败模型和重试，不调用 commit；
- 高级模式禁止逐模型混合，保存整个 Provider 的单协议；
- split 门面在 Provider 列表显示为一个逻辑源并可恢复编辑；
- 删除逻辑源不会留下受管叶子。

### 向导

- 复用正式深探测，不调用旧浅探测判断协议；
- 复用 Provider 配置组件与 Split/Blocked 组件；
- 不前端循环保存两个叶子；
- 最终批量事务失败零写入；
- split source 在向导中显示为一个逻辑源，最终 Router 持久化为叶子引用；
- 成功后历史修复、Subagent、推理强度、模型顺序入口仍存在。

### 迁移与运行时

- 新 Provider 即使目录含模型级 `apiFormat` 也被保存校验拒绝；
- 旧混合 Provider 只有 Verified profile 才可动态切换；
- 旧 profile 过期/不匹配时回落顶层协议并产生迁移诊断；
- 迁移完成后运行时不再走逐模型动态分支。

## 完成门禁

1. Provider Set 纯规划和事务测试全部通过；
2. protocol compatibility、Provider service、MultiRouter、Universal、forwarder 聚焦测试全部通过；
3. 前端 Provider/向导聚焦测试、全量 Vitest、typecheck、renderer build 通过；
4. Rust library 全量、`cargo check --tests --no-default-features`、rustfmt 通过；
5. Windows 应用构建通过，并在真实 Tauri 桌面完成 Single、Split、Blocked、split→uniform、向导和历史修复入口交互验收；
6. 严格 UTF-8 无 BOM/U+FFFD、`git diff --check` 通过；
7. `memory.md` 记录最终边界、测试证据、已知限制和联网检索来源；
8. 关键阶段分别本地提交，提交说明最后一行均为 `本次提交由BigStrongsSun完成`；未获授权不推送、不发布、不安装、不重启。
