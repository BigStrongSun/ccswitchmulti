# Codex 预设模型推理能力统一设计

## 目标

CCSwitchMulti 必须为 Codex 提供与真实上游一致的模型推理档位。用户选择内置预设 Provider 时，不需要理解厂商协议字段，也不需要手工修正生成的 `config.toml` 或 model catalog。自定义 Provider 和魔改中转仍允许用户声明或覆盖能力。

本设计同时覆盖单 Provider 与 MultiRouter，并以以下不变量作为验收标准：

> 同一次请求使用的模型菜单能力与出站转换能力，必须来自同一个最终解析结果。

## 问题根因

当前实现存在三套彼此独立的信息源：

1. `src/config/codexProviderPresets.ts` 声明预设模型和部分 `codexChatReasoning`。
2. `src-tauri/src/codex_config.rs` 从 GPT 或 Native Responses 通用模板生成目录，只对少数模型做硬编码覆盖。
3. `src-tauri/src/proxy/providers/codex.rs` 按 Provider 名称、URL 和模型名推断请求转换行为。

因此同一模型可能在 Codex 菜单显示一组档位，代理却折叠、忽略或改写为另一组值。MultiRouter 物化目标 Provider 后还可能丢失模型级能力，再次落入通用模板。

## 产品边界

### 内置预设

内置预设是 CCSwitchMulti 维护的 Codex 兼容适配器，不只是 URL 和模型名模板。每个已收录模型必须声明经官方资料或真实接口验证的能力；证据不足时采用保守能力，不得虚构 GPT 档位。

内置预设的基础能力默认只读。高级用户可以创建覆盖，但界面必须标记“已偏离内置预设”，并支持一键恢复。

### 自定义 Provider

自定义 Provider 可以：

- 选择一个兼容能力模板；
- 为每个模型编辑支持档位、默认档位、关闭能力及上游映射；
- 导入或导出高级 JSON；
- 不声明能力时使用保守兜底，而不是 GPT 四档兜底。

### 聚合平台

OpenRouter、SiliconFlow 等平台的协议能力优先于模型原厂协议。能力匹配维度必须包含 Provider 身份、API 格式和具体模型。聚合平台若提供模型能力发现接口，后续可刷新内置快照，但运行时不能依赖联网才能生成可用目录。

## 统一数据模型

前端持久化和 Rust 后端共享等价 schema。首版扩展已有 `modelCatalog.models[]`，为每个模型增加可选 `reasoning`，同时保留 Provider 级 `codexChatReasoning` 只用于迁移旧数据。

```ts
type CodexReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max";

interface CodexModelReasoningCapability {
  supported: boolean;
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
  source?: "builtin" | "user" | "legacy";
}
```

约束：

- `supported=false` 时目录不暴露推理档位，出站不发送强度。
- `supportedEfforts=[]` 可表示只有思考开关而没有强度档位。
- `defaultEffort` 必须属于 `supportedEfforts`。
- `disableAllowed=false` 时不得生成或发送 `none`。
- 字符串/对象强度模式必须为每个目录档位提供确定映射；缺失映射是保存错误，不允许运行时猜测。
- boolean 模式把关闭信号映射为 `false`，任意合法开启档位映射为 `true`，不声称不同开启档位具有不同强度。

## 能力解析与优先级

新增后端唯一入口 `resolve_codex_model_capability(provider, model)`，按以下顺序解析：

1. 模型级用户覆盖。
2. Provider 级用户覆盖。
3. 内置预设中该模型的能力。
4. 旧 `codexChatReasoning` 迁移结果。
5. API 协议保守兜底。

保守兜底规则：

- 未知 Native Responses 模型不自动继承 `none/high`；没有明确能力时不展示档位，也不覆盖上游默认值。
- 未知 ProxyChat 模型不自动继承 GPT 的 `low/medium/high/xhigh`。
- OpenAI 官方模型继续使用官方 Codex catalog，不经过第三方保守兜底。

解析结果包含来源和诊断信息，以便 UI 展示“内置”“用户覆盖”“旧配置迁移”“未知/保守”。

## 数据流

### 两层责任边界

推理强度适配必须明确拆成两个层面：

1. **CCS/CCSM 的多模型能力层**：维护每个“平台 + 精确模型”的真实能力，生成 Codex 可消费的模型目录，并在 Responses 转 Chat/Anthropic 等协议边界把 Codex effort 转为上游实际字段。该层负责能力证据、开关协议、档位映射和 fail-closed；不能要求 Codex 理解 Qwen、DeepSeek、OpenRouter 等厂商方言。
2. **Codex 的选择与发送层**：从 `ModelInfo.supported_reasoning_levels` 构造可选项，从 `default_reasoning_level` 与 `model_reasoning_effort` 解析当前线程值，最终统一发送 Responses `reasoning.effort`。Codex 只理解自己的 effort 枚举和每模型 catalog，不负责把它翻译成第三方参数。

因此“让 CCSM 正确认识模型能力”和“让 Codex 正确显示、保存并发送该模型的 effort”必须分别验收。前者正确但 catalog 投影错误，会导致 Codex 菜单错误；后者正确但代理映射错误，会导致菜单选择合法、上游请求却错误。

Codex catalog 是两层之间的正式契约：

- `supported_reasoning_levels` 是 Codex 可展示、继承和校验的集合；无分档模型必须明确为 `[]`。
- `default_reasoning_level` 只在存在合法默认档位时设置。
- `model_reasoning_effort` 是用户/线程/角色的选择，不是模型能力声明，也不能反向创造 catalog 中不存在的能力。
- Codex 发出的 `reasoning.effort` 仍是 Codex/Responses 语义；CCS/CCSM 只能在已经声明明确上游契约时翻译它。

### 预设保存

选择内置预设时，将预设 ID 和模型能力快照保存到 Provider 设置。预设升级后，未覆盖的内置能力可以随应用升级更新；用户覆盖只保存差异，不复制整份内置数据。

### Catalog 与 inline models

`codex_config.rs` 生成每个目录条目前先调用统一 resolver，再生成：

- `default_reasoning_level`；
- `supported_reasoning_levels`；
- Desktop camelCase aliases；
- `config.toml` provider inline `models` 三种兼容字段。

通用模板仍提供工具协议、上下文等非 reasoning 元数据，但其 reasoning 字段必须先清除，再由 resolver 明确写入。没有能力时不保留模板中的 GPT 档位。

### 请求转换

Proxy 收到 Codex 请求后，以路由物化后的 effective Provider 和真实 upstream model 调用同一个 resolver：

- 校验 Codex 发来的 effort 是否在可见集合内；
- 按 `effortMap` 转为上游值；
- 按 `parameter` 和 `format` 写入请求；
- 不支持 effort 时删除强度字段，只按声明处理思考开关；
- 非法值返回包含 Provider、模型、允许档位和能力来源的本地配置错误，不能静默猜测。

### MultiRouter

Route 引用内置 Provider 时，路由物化必须保留目标 Provider 的 preset identity、`modelCatalog` 和用户覆盖。最终能力使用 route 的 upstream model 解析，而不是可见 alias，也不能使用 MultiRouter 外层的 GPT 模板。

显式 route model map 后，resolver 输入必须是映射后的真实模型名。

## 内置预设首批校准

首批必须至少覆盖当前已确认不一致的预设：

- DeepSeek V4 Flash/Pro：`low/high/max`，默认 `high`。
- Grok 4.5：`low/medium/high`，默认 `high`，不可关闭。
- GLM-5.2：完整兼容枚举，默认 `max`，并按官方规则映射到 `none/high/max`。
- Step 3.7 Flash：`low/medium/high`；Step 3.5 Flash 2603：`low/high`；其他 Step 模型按官方证据逐项声明。
- OpenRouter：平台级七档输入能力与每模型能力分开处理；可获取模型元数据时使用模型快照，否则不宣称模型必然支持全部档位。

Kimi、Qwen、MiniMax、MiMo、SiliconFlow 等当前只有开关或不发送 effort 的路径必须显式声明。目录不能再展示实际上不会发送的多档强度。

## 配置界面

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

## 兼容与迁移

- 旧 Provider 没有模型级 `reasoning` 时，读取已有 `codexChatReasoning` 形成 `source=legacy` 的运行时能力。
- 重新保存内置预设时写入 preset identity 和用户差异，不把 legacy 推断固化成新官方事实。
- 保留旧字段读取至少一个稳定发布周期；新写入以模型能力 schema 为准。
- 现有用户手写 `config.toml` 不被 CCSwitchMulti 接管时保持不变。

## 错误处理

- 保存时拒绝默认档位不在支持列表、不可关闭却含 `none`、映射不完整、未知参数格式组合。
- 内置预设能力缺失视为开发期测试失败；发行构建不应把该模型作为“完全支持”展示。
- 用户覆盖无效时不回退到内置值并悄悄运行，而是保留原配置、显示具体校验错误。
- 运行时发现目录和请求能力摘要 hash 不一致时记录诊断，并以 effective Provider 的 resolver 结果拒绝非法请求。

## 测试与验收

### 单元测试

- 每个内置模型的 supported/default/disable/map 快照测试。
- resolver 五级优先级测试。
- catalog、camelCase、snake_case、inline TOML 投影一致性测试。
- Chat 转换对每个合法档位的出站字段测试。
- 非法配置和非法运行时 effort 的拒绝测试。

### MultiRouter 集成测试

- 内置 GLM route 的 visible alias 映射到 `glm-5.2` 后，菜单默认 max，请求 medium 映射 high。
- Grok 4.5 route 不出现 none，且 Native Responses 透传合法 effort。
- Step 两个模型在同一 Provider 下呈现不同档位。
- 只有 boolean thinking 的模型不出现虚假多档菜单。
- 用户模型级覆盖只影响指定 route/model。

### UI 测试

- 内置预设只读摘要、开启覆盖、恢复默认。
- 自定义 Provider 编辑和校验。
- 最终生效配置与保存后的后端解析结果一致。

### 完成标准

- 已收录预设不再依赖 GPT/Native 通用 reasoning 档位。
- catalog 和出站转换的能力摘要来自同一 resolver。
- 单 Provider 与 MultiRouter 对同一目标模型得到相同能力。
- 用户能够配置自定义 Provider，且能对内置预设进行明确标记的高级覆盖。
- 专项 Rust/前端测试、完整相关测试、typecheck、format check 和 `git diff --check` 全部通过。

## 官方依据与不确定性

设计依据来自 xAI、智谱、StepFun、OpenRouter 和 OpenAI 官方文档，并通过 Codex 内置 Web Search 与 Matrix WebSearch 两条独立链核对。Matrix 搜索索引查询未返回结果，但 Matrix 对官方 URL 的直接读取成功，内容与内置搜索一致。

Qwen 等平台的档位随具体模型和 API 形态变化，不能在缺少具体模型证据时写成厂商级固定枚举。该不确定性通过保守兜底和模型级能力声明解决，而不是继续使用通用档位。

2026-08-16 复核补充：Codex 当前源码把模型目录中的 `supported_reasoning_levels` 与可选菜单/换模校验绑定，把显式或默认 effort 序列化为 Responses `reasoning.effort`；`ultra` 在请求边界降为 `max`。CCS 官方当前已提供逐模型 `reasoningLevels/defaultReasoningLevel` 和多平台出站映射，但仍把空数组过滤成“未声明”，并保留模板档位，因此不能表达“已确认支持 reasoning、但无 graded effort”的完整能力；其目录声明与运行时 `codexChatReasoning` 也仍是两个数据结构。CCSM 的结构化单一 capability 方向更完整，但所有投影必须真正共用它，不能在 inline TOML 再引入通用默认值。
