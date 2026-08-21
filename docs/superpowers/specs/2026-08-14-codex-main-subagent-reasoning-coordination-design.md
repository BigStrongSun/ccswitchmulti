# Codex 主 Agent 与 Sub-Agent 推理强度协调设计

日期：2026-08-14
状态：事实模型已校正，等待产品策略评审；不得据旧版直接实施

## 1. 目标

让 CCSwitchMulti 用同一份已解析模型能力同时驱动：

- Provider 模型目录中的推理能力；
- Codex 主 Agent 模型选择器和当前线程推理强度；
- Sub-Agent V2 问卷、手工覆盖和生成的角色 TOML；
- `spawn_agent` 单次覆盖；
- Responses/Chat/Anthropic 上游请求转换。

用户需要把两个不同问题分开配置：

1. `能力来源`：自动发现或手动声明 Provider/模型支持哪些档位、默认值、关闭能力和映射；
2. `运行策略`：决定子 Agent 是继承父线程/单次 spawn、使用模型默认、固定某个档位，还是关闭推理。

`自动获取`只解决能力来源，绝不根据 Sub-Agent 任务描述自行选择并固定本次 effort。

Codex 完整选择词汇为：

```text
low, medium, high, xhigh, max, ultra
```

`none` 和 `minimal` 继续作为能力契约支持的特殊值，但不混入“从 low 到 ultra”的主强度下拉框；仅在模型允许关闭或明确支持 minimal 时单独展示。

## 2. 已确认的现状与根因

Provider 模型目录已经可以声明：

```text
supportedEfforts / defaultEffort / disableAllowed / upstream / effortMap / source
```

但 Sub-Agent 编译边界的 `CatalogModel` 当前只有：

```text
model / provider_kind / routable / context_window
```

推理能力在进入 Sub-Agent profile compiler 前丢失。Sub-Agent 前端、Rust 枚举和 `auto_effort()` 又维护了固定的 `auto/low/medium/high/xhigh`，既缺少 `max/ultra`，也不能按目标模型约束或映射。

当前生成的角色 TOML 总是写 `model_reasoning_effort`。这意味着子角色被固定后不会跟随主 Agent 对话框的当前 effort，且可能给 DeepSeek 等模型写入目录未声明的值。

## 3. 原生获取的真实边界

不存在跨 Provider 标准化的 reasoning-capability discovery API。常见 `/models` 接口通常只能列模型标识，不能可靠返回支持档位、默认值和参数映射。

因此，`自动获取` 不是单一 HTTP 调用，而是带来源和置信度的解析流程：

1. Provider 明确返回的能力元数据；
2. Codex 官方/原生模型目录中带来源的 `supported_reasoning_levels` 与 `default_reasoning_level`；
3. Provider 当前 `modelCatalog.models[].reasoning` 显式声明；
4. CCSwitchMulti 按官方资料维护的内置预设；
5. Provider/API 协议级兼容映射；
6. Codex 通用兼容回退；
7. 用户手工覆盖。

不得把 CCSwitchMulti 自己生成后又由 `codex debug models` 读出的目录当成新的 Provider 原生证据，否则旧错误会循环自证。`codex debug models` 和 app-server `model/list` 用于验证最终运行态，不用于提升来源置信度。

## 4. Codex 原生 Spawn 事实模型

当前 OpenAI Codex `main` 的真实顺序是：

1. 创建子 Agent 基础配置时，先复制父线程当前有效 model/provider；effort 使用父线程当前 `turn.reasoning_effort`，父线程也没有显式值时才使用父模型 catalog 默认值；
2. 应用 `spawn_agent.reasoning_effort`；未传时再读取 `[agents].default_subagent_reasoning_effort`；前者优先；
3. 如果 spawn 同时选择了另一个 model 且没有上述 effort，Codex 改用目标模型 catalog 的 `default_reasoning_level`；
4. 应用 agent role TOML；role 显式 `model_reasoning_effort` 时覆盖前述结果，未写时保留当前结果；
5. 对最终 model/effort 组合按目标模型 `supported_reasoning_levels` 校验；不支持则拒绝 spawn。

角色 TOML 是最后应用的配置层。只要角色文件显式写入 `model_reasoning_effort`，它就覆盖父线程、全局 Sub-Agent 默认和本次 spawn effort；这正是 CCSM 生成角色文件必须支持 `max` 的直接原因。

单次 spawn override 还存在运行版本差异：当前 OpenAI `main` 已在 full-history 路径应用 model/effort override，但本机当前 Codex 工具契约仍声明 full-history 继承 model/effort，显式 override 需要 fresh/partial fork。因此 CCSM 不能承诺所有 Codex 版本都支持 full-history 单次覆盖；它只负责省略角色固定值并让当前运行时按自身能力解析，同时在 UI/诊断中显示单次覆盖是否可用。

因此没有统一的“Sub-Agent 默认 high”。`[agents].default_subagent_reasoning_effort` 是可选值；本机 Codex 0.147.0 bundled catalog 中 `gpt-5.6-sol` 默认 `low`，Terra/Luna 默认 `medium`，其他模型也可以不同。

CCSM 当前 V2 与此原生模型冲突：其问卷 `auto_effort()` 根据任务类型算出 low/medium/high，且角色 TOML 总是写 `model_reasoning_effort`，结果是角色被锁定，主 Agent 无法在 spawn 时调节。

## 5. 统一能力契约

新增一个后端统一解析结果，前端只能消费该结果，不再自行猜测：

```json
{
  "model": "deepseek-v4-pro",
  "providerId": "deepseek",
  "protocol": "openai_chat",
  "reasoning": {
    "supportKind": "effort_levels",
    "source": "builtin_preset",
    "confidence": "confirmed",
    "codexSelectableEfforts": ["none", "low", "medium", "high", "xhigh", "max"],
    "providerAcceptedEfforts": ["low", "high", "max"],
    "providerDefaultEffort": "high",
    "disableAllowed": true,
    "disableMapping": {
      "openai_chat": {"parameter": "thinking.type", "value": "disabled"},
      "responses": {"parameter": "reasoning.effort", "value": "none"}
    },
    "effortMap": {
      "low": "low",
      "medium": "high",
      "high": "high",
      "xhigh": "high",
      "max": "max"
    },
    "ultraBehavior": "unresolved"
  }
}
```

关键点是分开表示：

- `codexSelectableEfforts`：目标模型目录实际允许 Codex spawn 校验通过的值；
- `providerAcceptedEfforts`：Provider 原生确认接受的值；
- `disableMapping`：关闭推理在不同上游协议中的独立表达；
- `effortMap`：从 Codex 强度意图转换为上游 effort 值，不混入关闭语义；
- `ultraBehavior`：`native`、`adapter_mapped`、`passthrough_unverified`、`unavailable` 或 `unresolved`。

已知只有思考开关的模型使用 `supportKind=boolean_only`；明确不支持的模型使用 `unsupported`；证据不足使用 `unknown`。

## 6. `ultra` 语义

最新版 Codex 原生 `ReasoningEffort` 已包含 `max` 和 `ultra`。`ultra` 同时参与 Codex 的主动多 Agent 行为，因此不能被当成所有第三方 API 都原生接受的普通字符串。

已经确认的事实：

- Codex 协议枚举接受 `ultra`，部分 Codex 模型 catalog 也会声明它；
- `ultra` 还与 Codex 主动多 Agent 行为有关；
- Codex 官方请求边界将内部 `ultra` 固定编码为 `reasoning.effort=max`；只有
  `multi_agent_version=v2` 时，Ultra 同时进入 proactive mode，注入允许主动
  委派 Sub-Agent 的 developer context；V1 的 Ultra 只会产生 `max`，不会主动委派；
- 因此 `ultra -> max` 是 Codex 产品层的固定转换，不是 Provider 原生映射；
- DeepSeek、Qwen/vLLM 等第三方 Provider 的能力声明只记录实际接受的
  `low/high/max`（或其它）值，绝不因启用 Ultra 而把 `ultra` 写进
  `providerAcceptedEfforts`；
- CCSM 另以 `codexUltraOrchestration.enabled` 表示用户已确认希望该模型参与
  Codex V2 Ultra 编排。启用的前提是存在经过校验的 `max -> Provider` 映射；
  它会把 `ultra` 加入 Codex catalog 的 `supported_reasoning_levels`，并在解析
  结果中记录 `ultra -> max/最高兼容目标`。

这使第三方模型可以使用“Codex Ultra 编排 + Provider 最高兼容 effort”，但它是
用户显式配置的 Codex 产品能力，不是自动发现出的 Provider 原生能力。界面必须把
它显示为独立的「Ultra（最大推理 + 主动 Sub-Agent 委派）」入口，并展示实际出站
路径；不得把它混入 Provider 原生档位复选框。

## 7. UI 设计

第一组控件是“模型推理能力来源”：

```text
自动发现 | 使用 CCSM 受维护声明 | 手动声明
```

它只回答：支持哪些 effort、默认是什么、能否关闭、如何转换到上游参数。

第二组控件是“Sub-Agent 运行时策略”：

```text
允许主 Agent / spawn 指定（推荐）
使用模型默认
固定档位
关闭推理
```

`允许主 Agent / spawn 指定`：

- 角色 TOML 省略 `model_reasoning_effort`；
- 同模型且 spawn 未指定时继承父线程当前 effort；
- 当前 Codex 运行时允许单次覆盖时，spawn 指定 effort 采用其值；不允许时按该运行时的 full-history 继承规则处理；
- `[agents].default_subagent_reasoning_effort` 仅在 spawn 未指定时生效；
- 角色切换到不同模型时，对最终值重新校验。

`使用模型默认`：

- 解析 catalog `default_reasoning_level` 并在角色中显式写入；
- 这会锁定为保存时解析出的默认值，主 Agent spawn 参数不能覆盖；
- UI 必须显示“固定为模型当前默认 X”，不能把它叫作动态 auto。

`固定档位`：

- 只显示目标模型 catalog 允许的档位；
- 写入角色 TOML，因此角色值最终优先；
- 同时显示 Provider 上游映射结果。

`关闭推理`：

- 只在 resolved capability 确认 `disableAllowed=true` 时展示；
- Codex catalog 必须包含 `none`，否则 spawn 原生校验会拒绝；
- Responses/OpenAI reasoning 映射为 `effort=none`；DeepSeek Chat 映射为 `thinking.type=disabled` 并省略无意义 effort；boolean-only Provider 映射为对应布尔关闭参数；
- 不支持关闭的模型不展示可操作开关。

能力 `自动发现`：

- 展示来源、置信度、Provider 原生档位、Codex 可选档位和映射；
- 基线使用 Provider/catalog 默认值；
- 不负责为角色选择本次 effort；
- 无可靠来源时转入未验证手动声明，而不是自动固定 medium/high。

`手动声明`：

- 编辑完整候选集合 `none/minimal/low/medium/high/xhigh/max/ultra/custom`；
- 每一项同时显示最终上游值；
- 原生支持、已映射、未验证透传使用不同状态文案；
- 非法或缺少映射不静默改值。

Provider 能力未知且没有用户覆盖时，可以在能力编辑器中提供完整 Codex 候选集，但不能把这些候选直接写入运行 catalog 并宣称支持；用户声明或探测通过后才进入目标模型的有效集合。

## 8. 主 Agent、角色与单次 Spawn 的优先级

最终优先级定义为：

```text
角色 TOML 显式 model_reasoning_effort
> spawn_agent.reasoning_effort
> [agents].default_subagent_reasoning_effort
> spawn 选择新模型时的目标 catalog 默认值
> 父线程当前 reasoning_effort
> 父模型 catalog 默认值
```

上述是当前 `main` 的源码执行结果，不是 CCSM 自定义偏好。角色未写 effort 时才允许后面的值保留下来；旧版/安装态 Codex 对 full-history override 的限制必须作为运行时能力差异单独展示。

界面必须明确显示策略：

- `固定 high`：主 Agent 下拉框不会影响该角色；
- `使用模型默认 -> high（固定）`：角色保存时写 high；
- `允许主 Agent / spawn 指定`：角色不写 effort，按 Codex 原生优先级解析；
- `关闭推理`：角色固定 none，并由 Adapter 翻译为 Provider 的关闭信号。

当前 Codex 原生实现会验证子 Agent 的 requested/role effort 是否存在于目标模型 `supported_reasoning_levels`。CCSwitchMulti 必须让主模型目录、Sub-Agent 编译器和出站转换共享同一解析结果，避免前一层允许、后一层拒绝。

## 9. 保存与运行态验证

保存事务必须完成：

1. 解析 Provider + 协议 + 具体模型能力；
2. 校验手工值和所有映射；
3. 编译角色 TOML；
4. 原子写入；
5. 回读并反序列化；
6. 对比预期的 model/effort/inherit 状态；
7. 重新生成并读取 Codex model catalog；
8. 使用 `codex debug models` 或 app-server `model/list` 验证主选择器运行态；
9. 使用真实 spawn 记录验证子 Agent 最终 model/effort；
10. 使用代理请求日志验证上游实际 effort。

连接探测是可选证据，不能假装所有 Provider 都支持无成本 capability probe。探测请求必须最小化、可取消，并记录 HTTP 状态和供应商错误；失败不能污染已确认预设。

## 10. 迁移

旧 profile 迁移规则：

- 旧 `auto` 不能直接继续解释为“任务算法算档位”；迁移为“允许主 Agent / spawn 指定”，并提示行为变化；
- 旧显式档位若有确定映射则迁移并显示映射说明；
- 旧值目标模型不支持且无映射时改为未验证手工值并阻止无提示保存；
- 新增结构化 `runtimePolicy=delegated|model_default|fixed|disabled`，不能用空字符串表达；
- 迁移后重新生成、回读 TOML，并返回逐角色警告。

## 11. 验收矩阵

至少覆盖：

- OpenAI 原生 GPT-5.6：完整目录包含 `low/medium/high/xhigh/max`，仅目录实际声明时显示 `ultra`；
- DeepSeek V4：可关闭，原生 `low/high/max`，`medium/xhigh -> high`，不虚构 ultra 映射；
- OpenRouter：`max/ultra` 按平台已声明规则映射到 `xhigh`；
- GLM、Grok、Step 2603：验证各自不同枚举和拒绝路径；
- SiliconFlow/boolean-only：只显示开关，不伪造 effort；
- unknown OpenAI-compatible：完整手工列表、未验证状态、透传错误可见；
- 主 Agent high + 子角色 follow_parent：子 Agent 为 high；
- 主 Agent high + 子角色固定 low：子 Agent 为 low；
- 主 Agent high + `[agents].default_subagent_reasoning_effort=low` + spawn 未指定：子 Agent 为 low；
- 主 Agent high + 全局子 Agent 默认 low + spawn 指定 max：子 Agent 为 max；
- role 固定 low + spawn 指定 max：最终仍为 role low；
- `3.19.1-25` 用户现场角色编辑选择 max：保存后角色 TOML 精确写 `model_reasoning_effort = "max"`，回读与预览一致；
- 支持 full-history override 的 Codex 与要求 fresh/partial fork 的 Codex 分别验证，禁止只验证一种运行时后宣称普遍可用；
- DeepSeek `disabled`：Codex catalog 接受 none，Chat 上游写 `thinking.type=disabled`；
- 主 Agent ultra + 不声明 ultra 的 DeepSeek role 继承：spawn 明确失败或要求选择兼容值，不静默改 max；
- 角色显式非法值：预览和保存都拒绝；
- 旧 profile 迁移、TOML 回读、真实 spawn 与上游请求日志一致。

## 12. 不采用的方案

- 只给固定枚举补 `max/ultra`：仍会把不受支持的值写给不同 Provider；
- 只在前端过滤：Rust 编译器和请求转换继续漂移；
- 把所有 Provider 都标成原生支持完整 Codex 档位：会混淆 UI 意图和上游真实能力；
- 读取 CCSM 自己生成的 Codex catalog 后反向当成 Provider 原生能力：会形成循环自证。
- 用旧 `auto_effort()` 按任务优势算 low/medium/high 并固定角色：这不是 Codex 原生 Sub-Agent auto 语义，会剥夺主 Agent 的 spawn 调节能力。

## 13. 调研证据

两条独立联网链均以当前一手来源交叉验证：Codex 内置搜索定位官方源码，Matrix WebSearch 独立打开并检索相同官方文件。Matrix 泛搜索没有返回有效结果，但其对官方 URL 的 `open/find` 成功，结论与本地 Codex 0.147.0 catalog 检查一致。

- OpenAI Codex 父配置继承与 spawn/default 处理：<https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents_common.rs>
- OpenAI Codex V2 spawn 调用顺序：<https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs>
- OpenAI Codex role TOML 高优先级覆盖：<https://github.com/openai/codex/blob/main/codex-rs/core/src/agent/role.rs>
- OpenAI Codex `[agents].default_subagent_reasoning_effort` 配置定义：<https://github.com/openai/codex/blob/main/codex-rs/config/src/config_toml.rs>
- DeepSeek 当前 Thinking Mode 文档：<https://api-docs.deepseek.com/guides/thinking_mode/>

当前证据仍未证明所有第三方 Provider 都能原生发现 reasoning capability，也未证明 DeepSeek 支持 `ultra`；这两项必须保持“证据不足/未解析”，不能用兼容推测冒充厂商能力。

分支边界：用户问题来自已发布 `3.19.1-25`；`3.19.1-26@dd967801` 正在打包且仍缺少角色 `max` 类型支持。本设计的运行代码必须基于 `bigstrongsun/release-v3.19.1-26` 创建下一版本分支实施，不能修改 `-26` 打包工作树，也不能继续落在旧 `-25` 分支。

## 14. 实施边界

本设计批准后，下一步先编写实施计划，再按测试驱动方式修改。实现应复用现有 Provider reasoning capability resolver，扩展其输出并传入 Sub-Agent compiler，不新增第二套模型名推断表。
