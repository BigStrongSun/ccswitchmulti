# Codex 模型元数据投影一致性审计（2026-08-20）

## 范围与结论

本审计覆盖官方与第三方模型从 CCSwitchMulti 路由设置到以下消费者的完整链路：生成 catalog、`model_catalog_json`、`models_cache.json`、活动 provider inline `models`、Codex Desktop CDP 注入以及 app-server `model/list`。

结论：完整 JSON catalog 与 CCSM-owned models cache 使用同一份 enriched catalog，当前现场三类代表模型在这两层字段一致；主要分叉发生在活动 provider inline `models`，以及 CDP 在 payload 中找不到模型描述时构造的通用 fallback descriptor。前者只投影 picker 字段子集，后者无证据地假定 `medium` 默认值和 `low/medium/high/xhigh` 四档推理能力。主 catalog 可读时多数元数据不会丢失，但 JSON/catalog 短暂不可读、热重载或默认模型不在 payload.models 时，官方与第三方模型都可能降级。

这也回答了“官方配置为什么会被写错、第三方是否也会遗漏”：CCSM 显式创建的官方 V2 profile 与第三方 V2 profile 当前都可编辑，并非官方只读；但本次 picker 故障的直接原因不是 profile 编辑器修改了官方模型能力，而是同一模型被投影到多份不同完备度的数据源。第三方同样会遗漏，而且纯文本模态、多 Agent 协议和未知推理能力的风险更高。

## 官方契约基线

OpenAI Codex 当前 `ModelInfo`/app-server `model/list` 不只包含模型名和推理档位，还包含 service tier、输入模态、模型 specialty、personality 支持、multi-agent version、升级/可用性提示等。公开 schema 中 `inputModalities` 缺失时默认 `['text', 'image']`；`additionalSpeedTiers` 已标记 deprecated，新的权威表达是 `serviceTiers`。Sub-Agent 运行时会使用模型 reasoning 与 service-tier 元数据进行参数校验。

因此不能把“能在 picker 里出现”视为元数据投影完成，也不能把字段缺失统一解释为“不支持”或“支持通用默认值”。

## 数据流与字段矩阵

| 元数据族 | enriched JSON catalog | CCSM-owned cache | provider inline models | CDP payload / descriptor | 审计判定 |
|---|---|---|---|---|---|
| identity / display / context | 保留，并补常用 alias | 与 enriched catalog 合并一致 | 有 | 复制 entry；缺 entry 时造最小描述 | 基本完整 |
| reasoning | 保留官方或用户声明 | 保留 | 有，含 snake/camel | 复制 entry；缺 entry 时伪造四档和 medium | fallback 不安全 |
| speed / service tier | 保留 | 保留 | `811433d6` 后已补；已安装旧版本仍缺 | 复制 entry | 源码已修，待安装验收 |
| input modalities | 同时写 snake/camel | 保留 | **缺失** | 完整 entry 时保留；inline fallback 时丢失 | P1 |
| multi-agent version | 同时写 snake/camel | 最终 cache 强制采用当前 V1/V2 | **缺失** | 完整 entry 时保留；inline fallback 时丢失 | P1/P2 |
| personality / specialty | 同 slug 官方字段可留在完整 entry | 保留 | **缺失** | 仅完整 entry 可留；无统一 alias 补齐 | P2/P3 |
| visibility / default | 保留路由可见性 | 保留 | 有 | CDP 强制 `hidden=false` | 符合模型解锁目的 |
| upgrade / availability NUX | CCSM 有意不应继承官方发布推广状态 | 同 catalog | 缺失 | 缺失 | 设计性差异，不建议复制 |
| runtime transport / tool / truncation | 同 slug 官方元数据优先保留 | 保留 | 大多缺失 | 仅完整 entry 可留 | 不应无差别塞入 inline；需按消费者分类 |

## Findings

### P1：inline fallback 丢失输入模态，字段缺失会被 Codex 默认成文本+图像

`codex_provider_models_toml_array()` 没有写 `input_modalities` / `inputModalities`。Desktop 的数据源顺序是主 catalog、CCSM-owned cache、active provider inline；正常主路径完整，但前两者不可读时会使用 inline。按照官方 schema，缺失 `inputModalities` 会默认成 `['text', 'image']`，因此已确认纯文本模型可能在 renderer/app-server 边界被错误宣称支持图片。

这不是纯展示问题：错误能力声明可能让客户端接受并发送模型不支持的输入。修正应从统一的 picker projection contract 生成 inline，而不是在 CDP 或 UI 对具体模型名打补丁。

### P1/P2：inline fallback 丢失 `multiAgentVersion`

完整 catalog 的 `apply_codex_multi_agent_transport_policy()` 写入 snake/camel 两种字段，最终写 cache 时还会让当前 V1/V2 选择覆盖旧官方备份；inline 却没有投影这两个字段。进入 inline fallback 后，模型可能失去其 V1/V2 transport 声明。

这会影响 Sub-Agent 工具结构选择，严重时可形成 reserved schema 不匹配；实际严重度取决于当前 Codex feature override 是否先行覆盖，因此归为高风险、需要 RED 集成测试确认的 P1/P2，而不是已证明每次都会触发的 P1 事故。

### P2：CDP fallback 无证据伪造通用推理能力

`descriptorFor()` 在找不到 payload 中同名 entry 时，硬编码 `defaultReasoningEffort='medium'` 和 `low/medium/high/xhigh`。Rust 的 `project_codex_model_descriptor()` 也会在缺值时补 `defaultReasoningEffort='medium'`。

对“未知能力”的正确语义应是未知并使用服务端默认，而不是四档支持。当前正常 projection 会为 `modelNames` 中每个 entry 同时生成 payload.models，所以主要触发条件是 default model 不在 routed entries、payload 形状异常或以后新增只传名字的调用方。应删除无证据的 capability fabrication，fallback 只补 identity/display/visibility；若消费者要求字段存在，应从同一 resolver 取得显式 supported/unsupported/unknown 结果。

### P2/P3：personality、specialty 与 alias 投影没有统一契约

同 slug 官方完整对象可通过 merge 保留新字段，但 `project_official_picker_metadata_aliases()` 只显式补 reasoning、speed/service、summary、verbosity 等少数 alias；inline 也不包含 `supportsPersonality`、`modelSpecialty`。主路径通常无影响，fallback 或 Desktop 直接消费 snake/camel 不一致的 payload 时会产生展示/功能降级。

应先用当前 app-server schema 建立 picker-public 字段白名单，再集中完成 alias normalization。`upgrade`、`upgradeInfo`、`availabilityNux` 不应因为“官方有”就复制到第三方或路由 catalog，它们是官方发布/升级状态而非模型固有能力。

### P2：源码修复与已安装运行态存在版本差

提交 `811433d6` 已把 `additional_speed_tiers` / `service_tiers` / `default_service_tier` 及 camelCase alias 投影进 inline，并保证第三方 service tier 为空，不继承 OpenAI Fast。当前磁盘现场的完整 catalog/cache 已保留这些字段，但已安装 CCSM 仍是提交前行为，不能用源码测试代替安装后的 Desktop 点击验收。

## 当前现场证据

2026-08-20 只读检查：

- `cc-switch-model-catalog.json`：9 个模型；`gpt-5.6-sol` 55 字段、6 个推理档、1 个 service tier、`text,image`、V2；`qwen3.8` 56 字段、3 个推理档、0 个 service tier、V2；`deepseek-v4-flash` 57 字段、3 个推理档、0 个 service tier、`text`、V2。
- `models_cache.json`：上述三模型的字段数量和关键值与完整 catalog 一致。
- 官方 backup 中 `gpt-5.6-sol` 为 35 字段，证明 enrichment 在保持官方能力的同时增加了 CCSM 路由投影；最终 cache 不是简单复制 inline。
- 当前分支工作区仅有用户已有的未跟踪 `.tmp/`，审计未改运行中 CCSM、未安装或覆盖程序。

`qwen3.8` 当前 catalog 声明 `text,image`，这只能证明当前配置最终值，不能仅凭模型名判断其部署实际支持图片；应继续由 provider capability/用户声明校验，而不是在本次审计中改写。

## 修正计划（尚未实施）

1. 定义单一 `PickerModelProjection` 契约，明确字段分组：identity、reasoning、service、modalities、multi-agent、personality/specialty、visibility；JSON、cache、inline、CDP 均从它投影。
2. 先写 RED 测试：纯文本第三方经 inline/CDP fallback 仍为 `['text']`；V2 模型保留 `multiAgentVersion='v2'`；unknown reasoning 不出现伪造四档；官方 service/personality 字段不丢；第三方不继承官方 Fast/upgrade/NUX。
3. 让 inline 至少包含 app-server `model/list` 的 picker-public 必要字段及 snake/camel 兼容 alias，不复制 runtime-only 大对象。
4. 把 CDP `descriptorFor()` 降为身份/可见性兜底；能力字段只能来自 payload 中经过 resolver 的明确结果。
5. 增加跨层快照测试，比较同一模型在 generated catalog、enriched JSON、cache、inline、CDP result 的等价字段，而不是分别维护零散断言。
6. 新构建安装后做真实 Desktop 验收：官方推理档、service tier/速度、第三方档位、纯文本图片入口、Sub-Agent V1/V2；验收前不得宣称运行态修复完成。

## 搜索与证据质量

本审计使用了两条独立联网链。Codex 内置搜索定位并交叉检查 OpenAI Codex 官方 `ModelInfo`、app-server README、`ModelListResponse` schema 和 Sub-Agent runtime 源码；Matrix WebSearch 同时执行，但返回结果质量不足，未提供比官方源码更强的一手证据。因此技术结论以 OpenAI 官方源码/schema和本地 CCSM 源码、当前磁盘数据为主。仍不确定的是不同 Codex Desktop 构建何时具体切换到 inline fallback，以及 feature override 对 `multiAgentVersion` 缺失的实际遮蔽程度，需靠安装后的跨版本集成测试确认。

官方一手资料：

- <https://github.com/openai/codex/blob/main/codex-rs/protocol/src/openai_models.rs>
- <https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md>
- <https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/json/v2/ModelListResponse.json>
- <https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents_common.rs>
