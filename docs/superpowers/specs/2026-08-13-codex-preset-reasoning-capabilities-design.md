# Codex 预设模型推理能力统一设计

## 目标

CCSwitchMulti 必须为 Codex 提供与真实上游一致的模型推理档位。用户选择内置预设 Provider 时，不需要理解厂商协议字段，也不需要手工修正生成的 `config.toml` 或 model catalog。自定义 Provider 和魔改中转仍允许用户声明或覆盖能力。

本设计同时覆盖单 Provider 与 MultiRouter，并以以下不变量作为验收标准：

> 同一次请求使用的模型菜单能力与出站转换能力，必须来自同一个最终解析结果。

## 第一部分：已确认知识

### 1. 推理能力不是一个布尔值

以下能力彼此独立，不能互相推导：

- 模型会产生 reasoning；
- Provider 允许显式开启或关闭 reasoning；
- Provider 接受分档 effort；
- Provider 接受 token budget 或其他非 effort 控制；
- Provider 返回 Codex 能识别的 reasoning 输出结构。

因此“未读到推理档位”不等于“不支持推理”。能力发现必须至少区分
`confirmed_supported`、`confirmed_unsupported` 和 `unknown`；控制形态必须独立区分
`none`、`boolean`、`graded`、`budget` 与 `unknown`。只有明确的否定证据才能写入
`confirmed_unsupported`，接口缺字段、探测失败或模型不在维护库中都只能得到 `unknown`。

### 2. Provider 能力发现的真实边界

不存在跨 Provider 通用且可靠的 reasoning-capability discovery API。常见 `/models`
只返回模型标识；即使返回扩展元数据，也可能受账号权限、网关版本、区域和模型 revision
影响。因此 Provider/API 返回的“平台 + 精确模型 + revision”能力是首选证据，但必须同时保存
`source`、`confidence`、`fetchedAt` 和匹配键，不能让一次失败探测永久覆盖用户声明或已确认快照。

当 Provider 没返回能力时，正确结果是 `unknown`，不是“不支持”。CCSM 可以用受版本控制的
常用模型能力库补足自动模式，但库的匹配必须包含平台、API 格式、精确模型 ID 和必要的版本；
不能仅按 `qwen`、`deepseek` 等品牌名推断，也不能把 CCSM 自己生成的 Codex catalog 回读成
Provider 原生证据，形成循环自证。

### 3. CCS 官方与 CCSM 当前状态

CCS 官方当前已有逐模型 `reasoningLevels/defaultReasoningLevel` 和多平台出站映射，但仍把
空数组过滤成“未声明”，并可能继续保留模板档位；catalog 声明与运行时
`CodexChatReasoningConfig` 也不是同一个能力对象。因此它仍不能无歧义表达“确认支持 reasoning，
但没有 graded effort”。

CCSM 已有 `CodexModelReasoningCapability`、统一 Rust resolver、catalog 投影和 Sub-Agent schema v2
的基础，但仍需审计所有投影与兼容入口是否真正使用同一解析结果。尤其要隔离 legacy 迁移口：
能力为 `unknown` 时，为兼容历史配置而暂时保留旧 fixed effort，不等于允许新配置绕过能力校验。

### 4. Codex 的模型级处理

Codex 把 `ModelInfo.supported_reasoning_levels` 作为模型可选值和运行时校验边界，把
`default_reasoning_level` 作为缺省值，最终通过 Responses `reasoning.effort` 发送。Codex 不理解
Qwen、DeepSeek、OpenRouter 或自建 vLLM 的厂商私有参数，因此第三方参数翻译属于 CCS/CCSM。

OpenAI 官方模型也证明不存在通用 effort 全集：GPT-5.6 支持
`none/low/medium/high/xhigh/max`，GPT-5.4 Pro 只支持 `medium/high/xhigh`。所以 CCSM 应尽量把
模型原生档位直接投影给 Codex；只有存在明确、可解释且可逆的 Adapter 映射时，才额外暴露
Codex 侧别名。映射必须显示给用户，不能静默 clamp。

### 5. Codex Sub-Agent 的实际优先级

当前 Codex 主线的 spawn 流程已由源码复核：

1. 子 Agent 配置先继承父线程当前 effort；父线程没有显式值时取父模型 catalog 默认；
2. 单次 `spawn_agent.reasoning_effort` 优先于 `[agents].default_subagent_reasoning_effort`；
3. 显式换模型且仍未指定 effort 时，改用目标模型 catalog 默认；
4. 随后应用角色 TOML；角色显式 `model_reasoning_effort` 是最后的配置覆盖层，省略则保留前值；
5. 最终值按目标模型 `supported_reasoning_levels` 校验，fallback 元数据除外。

`spawn_agent` 的字段是否可见还受 Multi-Agent 版本、fork 模式和隐藏元数据设置影响；因此不能把
“单次 spawn 总能传 effort”作为唯一产品入口。CCSM 已采用的角色运行策略
`delegated/model_default/fixed/disabled` 仍然合理：`delegated` 不写角色 effort，
`model_default` 固定写入目标模型默认，`fixed` 写入经目标能力校验的值，`disabled` 仅在目标能力
确认允许关闭时写入 `none`。

### 6. 已确认的本地根因

当前实现存在三套彼此独立的信息源：

1. `src/config/codexProviderPresets.ts` 声明预设模型和部分 `codexChatReasoning`。
2. `src-tauri/src/codex_config.rs` 从 GPT 或 Native Responses 通用模板生成目录，只对少数模型做硬编码覆盖。
3. `src-tauri/src/proxy/providers/codex.rs` 按 Provider 名称、URL 和模型名推断请求转换行为。

因此同一模型可能在 Codex 菜单显示一组档位，代理却折叠、忽略或改写为另一组值。MultiRouter 物化目标 Provider 后还可能丢失模型级能力，再次落入通用模板。

## 第二部分：CCSM 修正计划

### 1. 修正原则

1. Provider/API 能返回精确模型能力时优先使用，并投影到 Codex catalog。
2. 读不到能力时保持 `unknown`，不得自动降级为“不支持推理”。
3. 自动模式按“动态 Provider 元数据 → CCSM 常用模型能力库 → 平台协议能力 → unknown”解析；
   用户模型级覆盖始终优先，且任何低置信度自动结果都不能覆盖它。
4. Codex 侧优先暴露模型原生支持的档位；额外档位只能来自显式映射。
5. CCSM 同时允许配置主模型与 Sub-Agent 策略，但二者消费同一份 resolved capability。
6. `none` 只有在 Provider 明确声明关闭契约时才能翻译为关闭信号；否则省略厂商字段并保留服务端默认。
7. 所有来源、映射、默认值和不确定状态都必须在 UI 可见、可诊断、可恢复。

### 2. 产品边界

#### 内置预设

内置预设是 CCSwitchMulti 维护的 Codex 兼容适配器，不只是 URL 和模型名模板。每个已收录模型必须声明经官方资料或真实接口验证的能力；证据不足时采用保守能力，不得虚构 GPT 档位。

内置预设的基础能力默认只读。高级用户可以创建覆盖，但界面必须标记“已偏离内置预设”，并支持一键恢复。

#### 自定义 Provider

自定义 Provider 可以：

- 选择一个兼容能力模板；
- 为每个模型编辑支持档位、默认档位、关闭能力及上游映射；
- 导入或导出高级 JSON；
- 不声明能力时使用保守兜底，而不是 GPT 四档兜底。

#### 聚合平台

OpenRouter、SiliconFlow 等平台的协议能力优先于模型原厂协议。能力匹配维度必须包含 Provider 身份、API 格式和具体模型。聚合平台若提供模型能力发现接口，后续可刷新内置快照，但运行时不能依赖联网才能生成可用目录。

### 3. 统一数据模型

前端持久化和 Rust 后端共享等价 schema。首版扩展已有 `modelCatalog.models[]`，为每个模型增加可选 `reasoning`，同时保留 Provider 级 `codexChatReasoning` 只用于迁移旧数据。

```ts
type CodexReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "ultra";

interface CodexModelReasoningCapability {
  supportStatus:
    | "confirmed_supported"
    | "confirmed_unsupported"
    | "unknown";
  controlKind: "none" | "boolean" | "graded" | "budget" | "unknown";
  supportedEfforts: CodexReasoningEffort[];
  defaultEffort?: CodexReasoningEffort;
  disableAllowed: boolean;
  upstream: {
    format: "none" | "boolean" | "string" | "reasoning_object";
    parameter:
      | "none"
      | "thinking"
      | "enable_thinking"
      | "reasoning_split"
      | "reasoning_effort"
      | "reasoning.effort";
    effortMap?: Partial<Record<CodexReasoningEffort, CodexReasoningEffort>>;
  };
  outputFormat?:
    | "auto"
    | "reasoning_content"
    | "reasoning"
    | "reasoning_details"
    | "think_tags";
  source?: "provider" | "builtin" | "user" | "legacy" | "protocol";
  confidence?: "authoritative" | "verified" | "maintained" | "inferred";
  fetchedAt?: string;
  providerKey?: string;
  modelRevision?: string;
}
```

约束：

- `confirmed_unsupported` 时目录不暴露推理档位，出站不发送强度。
- `unknown` 不暴露未经确认的档位，但 UI 必须允许用户进入结构化声明流程。
- `supportedEfforts=[]` 可表示只有思考开关而没有强度档位。
- `defaultEffort` 必须属于 `supportedEfforts`。
- `disableAllowed=false` 时不得生成或发送 `none`。
- 字符串/对象强度模式必须为每个目录档位提供确定映射；缺失映射是保存错误，不允许运行时猜测。
- boolean 模式把关闭信号映射为 `false`，任意合法开启档位映射为 `true`，不声称不同开启档位具有不同强度。

### 4. 能力解析与优先级

新增后端唯一入口 `resolve_codex_model_capability(provider, model)`，按以下顺序解析：

1. 模型级用户覆盖。
2. Provider 返回的精确模型能力元数据。
3. CCSM 内置“平台 + API 格式 + 精确模型 + revision”能力库。
4. Provider 级用户覆盖或平台协议能力。
5. 旧 `codexChatReasoning` 迁移结果。
6. 保守 `unknown`。

保守兜底规则：

- 未知 Native Responses 模型不自动继承 `none/high`；没有明确能力时不展示档位，也不覆盖上游默认值。
- 未知 ProxyChat 模型不自动继承 GPT 的 `low/medium/high/xhigh`。
- OpenAI 官方模型继续使用官方 Codex catalog，不经过第三方保守兜底。

解析结果包含来源和诊断信息，以便 UI 展示“内置”“用户覆盖”“旧配置迁移”“未知/保守”。

### 5. 数据流

#### 两层责任边界

推理强度适配必须明确拆成两个层面：

1. **CCS/CCSM 的多模型能力层**：维护每个“平台 + 精确模型”的真实能力，生成 Codex 可消费的模型目录，并在 Responses 转 Chat/Anthropic 等协议边界把 Codex effort 转为上游实际字段。该层负责能力证据、开关协议、档位映射和 fail-closed；不能要求 Codex 理解 Qwen、DeepSeek、OpenRouter 等厂商方言。
2. **Codex 的选择与发送层**：从 `ModelInfo.supported_reasoning_levels` 构造可选项，从 `default_reasoning_level` 与 `model_reasoning_effort` 解析当前线程值，最终统一发送 Responses `reasoning.effort`。Codex 只理解自己的 effort 枚举和每模型 catalog，不负责把它翻译成第三方参数。

因此“让 CCSM 正确认识模型能力”和“让 Codex 正确显示、保存并发送该模型的 effort”必须分别验收。前者正确但 catalog 投影错误，会导致 Codex 菜单错误；后者正确但代理映射错误，会导致菜单选择合法、上游请求却错误。

Codex catalog 是两层之间的正式契约：

- `supported_reasoning_levels` 是 Codex 可展示、继承和校验的集合；无分档模型必须明确为 `[]`。
- `default_reasoning_level` 只在存在合法默认档位时设置。
- `model_reasoning_effort` 是用户/线程/角色的选择，不是模型能力声明，也不能反向创造 catalog 中不存在的能力。
- Codex 发出的 `reasoning.effort` 仍是 Codex/Responses 语义；CCS/CCSM 只能在已经声明明确上游契约时翻译它。

#### 预设保存

选择内置预设时，将预设 ID 和模型能力快照保存到 Provider 设置。预设升级后，未覆盖的内置能力可以随应用升级更新；用户覆盖只保存差异，不复制整份内置数据。

#### Catalog 与 inline models

`codex_config.rs` 生成每个目录条目前先调用统一 resolver，再生成：

- `default_reasoning_level`；
- `supported_reasoning_levels`；
- Desktop camelCase aliases；
- `config.toml` provider inline `models` 三种兼容字段。

通用模板仍提供工具协议、上下文等非 reasoning 元数据，但其 reasoning 字段必须先清除，再由 resolver 明确写入。没有能力时不保留模板中的 GPT 档位。

#### 请求转换

Proxy 收到 Codex 请求后，以路由物化后的 effective Provider 和真实 upstream model 调用同一个 resolver：

- 校验 Codex 发来的 effort 是否在可见集合内；
- 按 `effortMap` 转为上游值；
- 按 `parameter` 和 `format` 写入请求；
- 不支持 effort 时删除强度字段，只按声明处理思考开关；
- 非法值返回包含 Provider、模型、允许档位和能力来源的本地配置错误，不能静默猜测。

#### MultiRouter

Route 引用内置 Provider 时，路由物化必须保留目标 Provider 的 preset identity、`modelCatalog` 和用户覆盖。最终能力使用 route 的 upstream model 解析，而不是可见 alias，也不能使用 MultiRouter 外层的 GPT 模板。

显式 route model map 后，resolver 输入必须是映射后的真实模型名。

### 6. 内置常用模型能力库

首批必须至少覆盖当前已确认不一致的预设：

- DeepSeek V4 Flash/Pro：`low/high/max`，默认 `high`。
- Grok 4.5：`low/medium/high`，默认 `high`，不可关闭。
- GLM-5.2：完整兼容枚举，默认 `max`，并按官方规则映射到 `none/high/max`。
- Step 3.7 Flash：`low/medium/high`；Step 3.5 Flash 2603：`low/high`；其他 Step 模型按官方证据逐项声明。
- OpenRouter：平台级七档输入能力与每模型能力分开处理；可获取模型元数据时使用模型快照，否则不宣称模型必然支持全部档位。

Kimi、Qwen、MiniMax、MiMo、SiliconFlow 等当前只有开关或不发送 effort 的路径必须显式声明。目录不能再展示实际上不会发送的多档强度。

### 7. 配置界面

推理能力属于模型，不属于整个 Provider。唯一普通用户入口固定为：

```text
Provider 编辑 → 模型列表 → 编辑模型 → 推理能力
```

普通用户不需要理解 `supportsThinking`、`supportsEffort`、`thinkingParam`、
`effortParam`、`effortValueMode` 或 `outputFormat`。模型编辑器只提供以下互斥模式：

1. **自动检测（推荐）**：采用 CCSwitchMulti 对“平台 + 精确模型”的已验证预设；证据不足时进入保守模式，不猜测 GPT 通用档位。
2. **不支持推理**：不展示推理强度，也不发送 reasoning 参数。
3. **支持推理开关**：只允许“跟随服务端 / 开启 / 关闭”；只有存在明确上游关闭契约时才展示关闭。
4. **支持推理强度分档**：只展示该模型真实支持的档位和合法默认值。
5. **高级自定义**：为自定义网关或已验证的特殊部署编辑协议字段。

模型卡片必须显示最终生效摘要，例如：

```text
Qwen3.8 / vLLM
推理能力：支持推理，但上游未声明强度分档
控制方式：使用服务端默认行为
推理输出：reasoning_content
```

这种模型在 Codex 中不得出现 `low/medium/high/xhigh` 菜单。能返回 reasoning、
能开关 reasoning、能分档控制 reasoning 是三个独立能力，不能互相推导。

Provider 编辑页的“Codex 推理能力”区域承担模型能力摘要与批量诊断，不作为第二套配置源：

- 内置预设默认展示只读的模型能力摘要与来源。
- 点击某个模型的“编辑”动作进入上述唯一入口。
- 自定义 Provider 默认可编辑，并提供兼容模板选择。
- 显示最终生效配置，而不只显示用户差异。
- 保存前进行 schema 和语义校验。
- 提供“恢复内置默认值”；JSON 导入/导出只放在专家模式。

高级自定义使用结构化控件编辑上游参数类型、参数路径、支持档位、默认档位、
关闭能力、档位映射和推理输出字段。原始 JSON 仅作为专家路径，并在保存时执行相同校验。

所有模式最终只生成一个 `CodexModelReasoningCapability`。JSON catalog、provider inline
TOML、MultiRouter route 物化和请求转换均从这一 resolved capability 派生，不允许 UI 或
兼容投影维护第二份默认档位。尤其必须区分“字段缺失”和“明确空数组”：

- 缺失可以进入兼容迁移或保守推断；
- `supportedEfforts=[]` 是明确的无分档声明，任何投影都不得回退到通用档位；
- 没有合法分档时不得生成 `default_reasoning_effort=medium`。

首版不做在线自动探测写入；探测结果容易受临时网关、账号权限和模型版本影响。在线刷新可以作为后续功能，必须经过用户确认后才覆盖配置。

### 8. 主模型与 Sub-Agent 配置

主模型的用户选择继续由 Codex 线程设置承担，CCSM 负责让 catalog 只暴露合法集合，并把 Codex
effort 翻译为 Provider 原生字段。CCSM 不应另造一个与 Codex 线程状态竞争的“当前主模型强度”。

Sub-Agent 编辑器在每个目标模型下只提供四种运行策略：

- `delegated`：省略角色 TOML effort，让 Codex 继承父线程、单次 spawn 或全局默认；
- `model_default`：解析并固定目标模型默认值，适合希望角色行为稳定的用户；
- `fixed`：只允许 resolved `codexSelectableEfforts` 中的值，并显示到 Provider 原生值的映射；
- `disabled`：仅当 resolved capability 明确允许关闭时可选。

若 capability 为 `unknown`，推荐值是 `delegated`。普通新配置不得直接把通用 `medium/high`
写入角色 TOML；用户若确实验证过网关能力，应先在模型的“推理能力”入口建立手动声明，再选择
`fixed`。旧 schema 配置可在带明确 legacy 警告的兼容路径中保留，直到用户重新保存或完成迁移。

CCSM 还应显示“最终控制来源”：父线程、全局 Sub-Agent 默认、单次 spawn、目标模型默认或角色
固定值。这样可以解释为何同一个模型在主 Agent 与 Sub-Agent 中表现不同，而不要求用户理解
Codex 内部配置层。

### 9. AI、CLI 与配置文件接口

Reasoning 不能只有 GUI 入口。它必须作为 CCSM 全局 AI Configuration Plane 的一个领域，和
Provider、MultiRouter、Sub-Agent、MCP 等其他配置共用同一套后端查询、校验、mutation、回读与
审计服务。不得为 AI 再实现一套模型推断规则，也不得把直接编辑 SQLite、生成的 `config.toml`、
model catalog 或角色 TOML 作为推荐自动化接口。

Reasoning 领域至少提供以下稳定操作：

```text
ccsm reasoning inspect  --provider <id> --model <id> --output json
ccsm reasoning detect   --provider <id> --model <id> --output json
ccsm reasoning plan     --file <declaration.json> --output json
ccsm reasoning apply    --file <declaration.json> --expected-revision <n> --output json
ccsm reasoning validate --provider <id> --output json
ccsm reasoning export   --provider <id> --output json
ccsm reasoning reset    --provider <id> --model <id> --expected-revision <n> --output json
```

接口语义：

- `inspect` 同时返回 persisted declaration、resolved capability、Codex catalog projection、
  Provider-native projection、来源与警告；
- `detect` 默认只探测和缓存，不持久化；只有显式 `apply/accept` 才能把结果固化为用户确认配置；
- `plan`/`--dry-run` 执行与真实写入完全相同的 schema 和语义校验，输出将发生的差异但不写状态；
- `apply/reset` 使用 revision 或等价乐观并发条件，执行原子保存、派生产物重建和写后回读；
- stdout 在机器模式只输出版本化 JSON，诊断写 stderr，退出码和错误码保持稳定；
- 所有输出默认脱敏，禁止返回 API key、OAuth token、Cookie 或原始凭据；
- mutation 返回 `changed`、新 revision、最终 resolved 结果、派生产物验证、是否需要重启 Codex、
  warnings 和 rollback/恢复标识；重复提交相同目标状态应幂等。

公开声明文件使用独立、版本化 schema，不直接暴露数据库行结构。例如：

```json
{
  "schemaVersion": 1,
  "kind": "ccsm.reasoning-capability",
  "providerId": "my-vllm",
  "model": "qwen3.8",
  "spec": {
    "supportStatus": "confirmed_supported",
    "controlKind": "graded",
    "supportedEfforts": ["low", "medium", "high"],
    "defaultEffort": "medium",
    "disableAllowed": false,
    "source": "user"
  }
}
```

AI 在没有 Provider 证据、维护库匹配或用户声明时只能保留 `unknown/server_default`，不能把猜测
写成 `confirmed_supported`。若用户希望 fixed effort，AI 应先 `inspect`，再提交模型能力声明的
`plan`，最后在 revision 未变化时 `apply`；随后才能配置 Sub-Agent fixed 策略。

CLI、未来本地 MCP/JSON-RPC、配置文件导入和 GUI 都只是 transport adapter。它们必须调用同一个
Reasoning Application Service；最终成功以数据库回读、resolver 结果和派生 Codex 配置验证为准，
不能以命令返回 0 或文件写入成功代替。全局 AI Configuration Plane 的范围、权限、命令树和分期
实施由独立设计文档进一步定义，本设计只约束 reasoning 领域契约。

### 10. 兼容与迁移

- 旧 Provider 没有模型级 `reasoning` 时，读取已有 `codexChatReasoning` 形成 `source=legacy` 的运行时能力。
- 重新保存内置预设时写入 preset identity 和用户差异，不把 legacy 推断固化成新官方事实。
- 保留旧字段读取至少一个稳定发布周期；新写入以模型能力 schema 为准。
- 现有用户手写 `config.toml` 不被 CCSwitchMulti 接管时保持不变。

### 11. 错误处理

- 保存时拒绝默认档位不在支持列表、不可关闭却含 `none`、映射不完整、未知参数格式组合。
- 内置预设能力缺失视为开发期测试失败；发行构建不应把该模型作为“完全支持”展示。
- 用户覆盖无效时不回退到内置值并悄悄运行，而是保留原配置、显示具体校验错误。
- 运行时发现目录和请求能力摘要 hash 不一致时记录诊断，并以 effective Provider 的 resolver 结果拒绝非法请求。

### 12. 测试与验收

#### 单元测试

- 每个内置模型的 supported/default/disable/map 快照测试。
- resolver 六级优先级测试。
- catalog、camelCase、snake_case、inline TOML 投影一致性测试。
- Chat 转换对每个合法档位的出站字段测试。
- 非法配置和非法运行时 effort 的拒绝测试。

#### MultiRouter 集成测试

- 内置 GLM route 的 visible alias 映射到 `glm-5.2` 后，菜单默认 max，请求 medium 映射 high。
- Grok 4.5 route 不出现 none，且 Native Responses 透传合法 effort。
- Step 两个模型在同一 Provider 下呈现不同档位。
- 只有 boolean thinking 的模型不出现虚假多档菜单。
- 用户模型级覆盖只影响指定 route/model。

#### UI 测试

- 内置预设只读摘要、开启覆盖、恢复默认。
- 自定义 Provider 编辑和校验。
- 最终生效配置与保存后的后端解析结果一致。

#### AI/CLI 契约测试

- `inspect` 的 persisted/resolved/Codex/Provider 四层输出与 GUI 使用的后端结果完全一致。
- `detect` 默认无持久化副作用，显式接受后写入且不会被下一次失败探测覆盖。
- `plan` 与 `apply` 使用同一校验；过期 revision 拒绝写入并返回稳定冲突错误码。
- 相同目标状态重复 apply 幂等，敏感字段在 stdout、stderr 和审计日志中均被脱敏。
- mutation 成功必须包含数据库回读、catalog/inline model 投影以及受影响角色文件验证。

#### 完成标准

- 已收录预设不再依赖 GPT/Native 通用 reasoning 档位。
- catalog 和出站转换的能力摘要来自同一 resolver。
- 单 Provider 与 MultiRouter 对同一目标模型得到相同能力。
- 用户能够配置自定义 Provider，且能对内置预设进行明确标记的高级覆盖。
- 专项 Rust/前端测试、完整相关测试、typecheck、format check 和 `git diff --check` 全部通过。

### 13. 官方依据与不确定性

设计依据来自 xAI、智谱、StepFun、OpenRouter 和 OpenAI 官方文档，并通过 Codex 内置 Web Search 与 Matrix WebSearch 两条独立链核对。Matrix 搜索索引查询未返回结果，但 Matrix 对官方 URL 的直接读取成功，内容与内置搜索一致。

Qwen 等平台的档位随具体模型和 API 形态变化，不能在缺少具体模型证据时写成厂商级固定枚举。该不确定性通过保守兜底和模型级能力声明解决，而不是继续使用通用档位。

2026-08-16 复核补充：Codex 当前源码把模型目录中的 `supported_reasoning_levels` 与可选菜单/换模校验绑定，把显式或默认 effort 序列化为 Responses `reasoning.effort`；`ultra` 在请求边界降为 `max`。CCS 官方当前已提供逐模型 `reasoningLevels/defaultReasoningLevel` 和多平台出站映射，但仍把空数组过滤成“未声明”，并保留模板档位，因此不能表达“已确认支持 reasoning、但无 graded effort”的完整能力；其目录声明与运行时 `codexChatReasoning` 也仍是两个数据结构。CCSM 的结构化单一 capability 方向更完整，但所有投影必须真正共用它，不能在 inline TOML 再引入通用默认值。
