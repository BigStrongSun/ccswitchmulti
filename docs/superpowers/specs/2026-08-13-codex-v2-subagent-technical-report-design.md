# Codex V2 Sub-Agent 技术科普与 CCSM 第三方模型适配报告设计

日期：2026-08-13

## 1. 交付目标

本项目交付一套内容同源、表现形式不同的技术材料：

1. 重写 `v3.19.1-23` GitHub Release Note，使它准确说明本版修复，并链接完整技术报告。
2. 新增一份中文 Markdown 长文，作为 Codex Sub-Agent V1/V2、V2 原生机制和 CCSwitchMulti 第三方模型适配的权威技术报告。
3. 使用 `html-ppt` 的 `document-deck` 模板，把长文转化为可独立阅读的 HTML 技术课件；它不是演讲提纲，读者不看 notes 也能理解完整因果链。

材料不以运维人员为读者，不写安装值守流程。目标读者是希望理解 Codex 多 Agent 机制、V2 的设计动机、第三方模型兼容障碍和 CCSM 实现原理的开发者与技术爱好者。写作采用“先建立直觉，再进入源码和协议”的深入浅出方式。

## 2. 核心叙事

整份报告按一条不可跳步的因果链展开：

```text
单 Agent 的上下文与执行瓶颈
→ Sub-Agent 在 Codex 中的意义
→ V1 的模型与生命周期
→ V1 为什么不足以承载新版协作
→ V2 的任务路径、mailbox 与 follow-up
→ V2 原生角色、工具、线程和消息结构
→ 能力描述为什么承担语义调度
→ 原生 V2 为什么无法直接调用第三方 child
→ CCSM 如何同时修复 schema、任务正文、路由和协议边界
→ 新角色从问卷到 TOML、再到真实 child 的完整运行
→ 踩坑、失败方案、根因与最终工程边界
```

报告必须区分三类事实：

- Codex 官方机制：由 OpenAI 官方源码、配置结构和官方 issue 支持。
- CCSM 实现：由当前仓库源码、测试和提交历史支持。
- 运行观察或仍存边界：明确标注验证环境，不把推断写成官方设计。

## 3. Markdown 报告结构

建议文件：`docs/codex-v2-subagent-third-party-adaptation-zh.md`。

### 第一章：为什么 Codex 需要 Sub-Agent

- 单 Agent 同时承担阅读、搜索、实现、测试和审查时的上下文竞争。
- Sub-Agent 不是“再调用一次模型”，而是一个有独立线程、上下文、工具权限和生命周期的执行单元。
- 父 Agent 的职责是分解、选角、收集结果与最终整合；child 的职责是完成边界明确的子任务。
- 用一个“定位跨模块配置回归”的任务展示串行单 Agent 与并行子 Agent 的差别。

### 第二章：V1 Sub-Agent 如何工作

- V1 的 `spawn_agent / send_input / resume_agent / wait_agent / close_agent` 工具族。
- agent id、最大深度、显式模型覆盖和父子生命周期。
- V1 的优势：机制直接、异构模型 payload 是标准 message、显式覆盖易理解。
- V1 的限制：围绕单个 agent id 管理，任务组织与 follow-up 语义较弱；模型候选描述不等于角色能力目录；规模扩大后父 Agent 需要承担更多手工调度信息。
- 给出 V1 结构图和一次请求时序图。

### 第三章：为什么出现 V2

- V2 从“启动一个编号 child”转向“创建一个有路径和身份的协作任务”。
- 解释 task path、mailbox、`send_message`、`followup_task`、`interrupt_agent`、`list_agents` 和并发线程限制。
- `fork_turns` 如何决定继承多少历史；full-history fork 与异构 override 的限制。
- V2 改善的是协作模型和任务生命周期，不是简单增加更多模型参数。

### 第四章：Codex 原生 V2 的完整结构

- 配置层：models catalog 的 `multi_agent_version`、`features.multi_agent_v2`、`[agents]` 与 `~/.codex/agents/*.toml`。
- 调度层：父模型看到 role description 后进行语义选角，`agent_type` 绑定 custom role。
- 工具层：reserved/non-reserved namespace、隐藏 metadata 后的官方 schema。
- 线程层：parent thread、child thread、parent turn、task path、activity item 和 mailbox。
- Provider 层：role 中的 model/model_provider/reasoning 决定 child runtime。
- 消息层：V2 `agent_message`、加密协作参数、child 输入与 parent 回收结果。
- 输出一张分层结构图、一张从配置加载到 child 完成的完整时序图。

### 第五章：为什么 V2 需要能力描述

- `task_name` 只是任务路径，不是角色选择器。
- 隐藏 metadata 时，父模型无法在每次 spawn 中安全暴露 model/reasoning；直接扩大 reserved schema 会在 OpenAI 后端 pre-reasoning 阶段被拒绝。
- custom role 的 `description` 是父 Agent 的语义路由表：说明“擅长什么、排除什么、何时优先”。
- `developer_instructions` 是 child 被选中后的执行策略，不负责父 Agent 的选角。
- model、provider、reasoning 是运行绑定；nickname 是显示信息。五者不能混为一谈。
- 用“仓库探索”和“复杂架构审查”两个任务说明相同模型名与不同能力描述会导致怎样的角色选择差异。

### 第六章：原生 V2 为什么不能直接使用第三方模型

必须拆成四个独立障碍，避免用“协议不兼容”一笔带过：

1. **Reserved schema 障碍**：暴露 `model/reasoning_effort/service_tier` 会改变 `collaboration.spawn_agent` 的保留 schema，官方后端在模型推理前返回 400。
2. **任务正文加密障碍**：V2 对 `spawn_agent/send_message/followup_task` 的 message 使用加密标记；第三方 child 没有 OpenAI 解密能力。
3. **私有 input item 障碍**：Codex 将任务封装为 `agent_message`；第三方 Responses API 通常只理解标准 `message`，Chat API 更需要继续转换为 user message。
4. **跨 Provider 回放障碍**：reasoning、web search call、encrypted content 和 Responses/Chat 历史对象不能无条件跨 transport 原样重放。

给出失败请求剖面图和“在哪里失败”的分层故障图，明确 V1 可用不代表 V2 可用。

### 第七章：CCSM 的第三方 V2 适配架构

分成控制面和数据面。

控制面：

- MultiRouter model catalog 提供可路由模型和能力。
- `subagentV2` 问卷保存用户语义意图，backend compiler 生成 role。
- role TOML 绑定 `codex_model_router_v2`，而不是直接绑定某个静态外部 endpoint。
- CCSM 保留官方 reserved schema，混合路由使用非保留 `agents` namespace 与 `hide_spawn_agent_metadata=true`。
- live projection 同步 config、catalog 与 CCSM-owned role 文件，并保护用户自写 role。

数据面：

- official parent 请求进入 CCSM proxy。
- 仅对混合路由中的非保留协作工具去除 message 的 encrypted schema 标记，使官方父模型返回可投递明文；纯 official 流程继续保留加密。
- Codex 创建 child 后，根据 role 的 model 和统一 router provider 发起请求。
- MultiRouter 按 exact/prefix/default route 解析，物化真实 Provider、认证、协议和 modelMap。
- 只有实际第三方 Responses child 才把 plaintext `agent_message` 投影为标准 user `message`；仍是 opaque ciphertext 时 fail closed。
- Chat/Anthropic 路径继续经过各自转换器；官方 OAuth 路径保持原生语义。
- child 结果通过 Codex 自己的 V2 生命周期回到父 Agent，CCSM 不在数据库保存 prompt、reasoning 或 response 正文。

输出一张 CCSM 加入前/后的对照结构图、一张控制面图、一张数据面时序图。

### 第八章：新增 V2 角色的完整运行过程

以 DeepSeek Flash 型只读探索角色为主案例，逐步展示：

1. 用户填写任务强项、优化目标、写入范围、偏好和 reasoning。
2. compiler 生成 role name、description、developer instructions、nickname、model/provider/reasoning。
3. 冲突检测与 `ccswitch-<role>` 安全重命名。
4. 写入 `~/.codex/agents/<role>.toml`，更新 catalog/config 投影。
5. 新会话中父 Agent 读取角色描述并按任务语义选择角色。
6. `spawn_agent` 创建 V2 child，任务正文经过 mixed-router 明文兼容路径。
7. role 选择的 model 进入 MultiRouter，物化第三方 Provider。
8. `agent_message` 投影、Responses/Chat 转换、真实上游调用。
9. child 使用工具完成任务，结果回到 parent，parent 做最终整合。

同时给出 Pro 型复杂实现角色的差异表。代码页只展示决定机制的最小真实片段，每行附中文机制说明，并在下方解释真实副作用发生在哪一层。

### 第九章：踩过的坑与错误方案

- 修改 reserved spawn schema：为何 400，为什么不能靠加字段解决。
- 只切换 namespace：为何解决 schema 但不能解决 encrypted payload。
- 把 task name 当 role name：为何不会选择 custom role。
- 只生成 TOML、不验证 child rollout：为什么配置存在不等于真实选型。
- V2 role 直接绑定第三方 provider：为什么会绕过 MultiRouter 的认证、协议和路由能力。
- 对所有 provider 投影 agent_message：为什么会破坏 Official OAuth。
- 发送 opaque ciphertext 或空 Payload：为什么必须 fail closed。
- 按模型名推断 official/third-party：为什么必须使用 Provider record 与运行时物化结果。
- 相对 `model_catalog_json` 与并发 alias 重复：为何新版 Codex 读取失败，以及 V23 如何根修。
- 非 current Provider 与 Official managed roles 的归因误区。

### 第十章：边界、结论和延伸阅读

- CCSM 没有重新实现 Codex orchestrator；它补齐配置编译、角色投影、路由物化和跨 transport 兼容层。
- 能力描述是 best-effort 语义调度，不是硬编码模型路由。
- 加密内容不可由 CCSM 解密；兼容依赖父请求在非保留 mixed-router 工具路径生成明文。
- 官方到官方继续保持原生加密与 OAuth；第三方兼容只在目标 Provider 已知后发生。
- 列出官方源码、官方 issue、CCSM 源码路径、关键提交和测试锚点。

## 4. HTML 技术课件设计

### 模板与风格

- 基础模板：`templates/full-decks/document-deck/`。
- 默认主题：`engineering-whiteprint`；允许按 `T` 切换 `academic-paper` 与 `tokyo-night`。
- 画面为工程白图纸风：坐标网格、深蓝墨线、有限橙色强调。视觉元素用于表达层级和数据流，不添加无信息装饰。
- 预计 24–30 页，页数由正文、图和代码可读性决定，不为控制页数压缩字号。

### 页面序列

1. 封面：从 V1 到 V2，以及 CCSM 如何让第三方模型成为 V2 child。
2. 阅读地图与事实标记。
3. 单 Agent 的上下文竞争。
4. Sub-Agent 的定位与父子职责。
5–7. V1 概念、结构图、时序图。
8–10. V2 动机、协作模型、V1/V2 对照。
11–14. Codex V2 配置结构、角色结构、运行结构、完整时序。
15–16. 能力描述的语义调度与实例。
17–20. 第三方失败的四层障碍、失败时序与请求剖面。
21–24. CCSM 控制面、数据面、前后对照与完整时序。
25–27. 新角色问卷到 child 运行的案例、TOML/源码和执行轨迹。
28–29. 踩坑地图与 V23 配置兼容复盘。
30. 实现边界、结论和源码索引。

### 图形清单

- 父 Agent / child Agent 职责分工图。
- V1 树状生命周期图。
- V1 调用时序图。
- V2 task path + mailbox 协作图。
- Codex V2 六层结构图。
- Codex V2 原生完整时序图。
- description / developer instructions / runtime binding 三段式角色图。
- 第三方失败四层漏斗图。
- 原生 V2 与 CCSM 适配后结构对照图。
- CCSM 控制面结构图。
- CCSM mixed-router 数据面完整时序图。
- 新角色从问卷到 child 的状态流水线。
- 踩坑与修复矩阵。

图形优先使用内联 SVG、CSS grid 和 HTML 表格，以保证离线可读、清晰缩放和文字可搜索。每张图都有结论型 caption，并标注对应源码/issue 锚点。

### 交互深度

- `agent_message`、reserved schema、role、Provider materialization、Responses、Chat bridge、fork、mailbox 等术语做成橙色交互术语。
- 悬停/聚焦显示短解释，点击固定，Esc 关闭；基础页面不依赖弹窗才能理解。
- 架构模块可点击进入 exploration 子页面，展示对应配置、源码和请求样例。

## 5. Release Note 重写原则

Release Note 保持发布导向，不把整份技术报告复制进去：

- 开头明确 V23 修复的两个启动阻断及影响。
- 用一节解释这些问题位于 V2 role/catalog 投影链，而非第三方 payload 兼容本身。
- 加入“为什么这个版本重要”：V2 第三方 child 依赖可解析的 catalog/config/role 投影。
- 保留升级步骤、完整退出 Codex 并新建会话的说明。
- 增加完整技术报告和 HTML 课件链接。
- 更新 GitHub Release 正文时保留 workflow 自动添加的多平台下载区。

## 6. 证据与引用边界

主要外部证据：

- OpenAI Codex 官方 `multi_agents_v2/spawn.rs`、配置结构和工具 schema 源码。
- OpenAI Codex issues `#32674`、`#33551`、`#32705`、`#33267`、`#32753` 与 `#28058`，分别支持 reserved schema、第三方 agent_message、V2 runtime、加密输出和审计边界。
- 外部 issue 只作为复现与现状证据；设计意图必须由官方源码或明确官方说明支持。

主要本地证据：

- `src-tauri/src/codex_subagent_profiles.rs`：问卷、编译、description、instructions、reasoning 和 role 命名。
- `src-tauri/src/codex_config.rs`：catalog/config/role 投影、V1/V2 选择、schema 安全和诊断。
- `src-tauri/src/proxy/providers/codex_multi_agent.rs`：第三方 `agent_message` 投影。
- `src-tauri/src/proxy/forwarder.rs` 与 Provider materialization 链：只对真实第三方目标执行兼容转换。
- `docs/superpowers/specs/2026-08-05-codex-cross-provider-v2-subagent-payload-design.md`：跨 Provider payload 设计。
- 从 `v3.19.1-5` 到 `v3.19.1-23` 的测试、修复提交和 `memory.md` 验收记录。

Codex 内置 Web 搜索获得了官方源码与相关 issue。独立 Matrix WebSearch 没有返回等价的一手结果，因此不把其泛化结果作为关键事实依据；报告会如实注明两条搜索链的证据质量差异。

## 7. 验收标准

### 内容

- 从不了解 Sub-Agent 的读者可以按顺序理解 V1、V2 和 CCSM 适配，不需要先阅读源码。
- 每个“为什么”都有机制解释，每个关键机制都有结构图或时序图，每个工程结论都有证据锚点。
- 不把 V2 描述为 V1 的简单升级，不把 description 描述为硬路由，不声称 CCSM 能解密 OpenAI ciphertext。
- 清楚区分官方原生能力、CCSM 兼容层和仍存在的上游边界。

### Markdown

- 链接、标题层级、Mermaid/内联图、代码块与表格渲染正常。
- 源码路径、版本、提交和 issue 链接可追溯。
- 无 `TODO/TBD`、无占位图、无安装运维叙事漂移。

### HTML

- 桌面视口逐页渲染，无正文、代码、caption 或 footer 裁切。
- 关键页在较小窗口仍能滚动或自适应，不以缩小到不可读解决密度问题。
- 键盘导航、overview、主题切换、术语交互和 exploration 页面可用。
- 所有代码行有中文机制说明，代码下有整体控制流解释。
- 截图验收至少覆盖封面、V1 结构、V2 六层结构、第三方失败、CCSM 数据面、新角色流水线与踩坑页。

### Release

- GitHub Release 仍为正式非 prerelease，原有资产不变。
- 更新后的正文包含完整技术报告与 HTML 课件链接，下载区不丢失。
- 文档提交推送后，链接指向 tag/稳定分支上可访问的文件。

## 8. 非目标

- 不修改 V1/V2 运行代码、路由实现或数据库。
- 不新增新的 V2 兼容补丁；本项目解释当前已经实现并验证的机制。
- 不制作面向运维的安装手册、告警值班流程或数据库修复教程。
- 不把课件做成营销宣传、产品发布会或依赖讲者口播的幻灯片。
