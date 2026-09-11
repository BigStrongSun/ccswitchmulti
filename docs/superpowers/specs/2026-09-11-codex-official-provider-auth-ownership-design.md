# Codex OpenAI Official 认证所有权与独立账号池设计

日期：2026-09-11  
状态：已获用户方向批准，待实施

## 问题

当前 Desktop、CCSM 托管 OAuth 和 OAuth 账号池的选择保存在每个 MultiRouter 的
`settingsConfig.codexRouting.officialAuth`，保存时又物化到官方 route 的
`authPolicy`。入口藏在 MultiRouter 卡片悬浮操作的“重命名/设置”中，并在页面下方
行内展开。

这带来两个根本问题：

- 认证策略属于 `OpenAI Official` Provider，却由 MultiRouter 持有和编辑；多个
  Router 可以复制出互相冲突的官方认证策略。
- 账号池运行时只在 Router route 物化出 `codexAccountPoolEnabled`，独立启用
  `OpenAI Official` 时仍固定使用 Desktop 当前登录，导致账号池错误依赖 MultiRouter。

## 目标

- `OpenAI Official` Provider 是官方认证策略的唯一事实源。
- 关闭或完全不创建 MultiRouter 时，官方 Provider 接管仍可使用 Desktop、指定的
  CCSM OAuth 账号或 OAuth 账号池。
- MultiRouter 只负责模型到 Provider 的路由；引用 `OpenAI Official` 的 route 默认
  继承该 Provider 的认证策略。
- 入口直接、常显、符合用户对 Provider 配置归属的预期。
- 不复制 Token，不改写 Codex `auth.json`，不降低 Desktop Authorization 的隔离边界。

## 所有权

| 对象 | 持有内容 |
| --- | --- |
| `OpenAI Official` Provider | 官方认证模式，以及固定 CCSM OAuth 模式下的非秘密账号 ID |
| OAuth 认证存储 | access/refresh/id token、账号有效性、默认账号 |
| OAuth 账号池策略 | 启用账号、顺序、保留额度、Desktop 成员身份 |
| MultiRouter route | 目标 Provider、模型选择、别名和匹配规则；默认不持有官方认证模式 |
| Codex live 门面 | 根据当前 Provider 认证策略和账号池成员动态生成的运行时投影 |

## Provider 配置

在 `codex-official` 的非秘密元数据中增加显式官方认证配置：

```json
{
  "codexOfficialAuth": {
    "mode": "desktop_current_login | managed_oauth | account_pool",
    "accountId": "仅 managed_oauth 可选"
  }
}
```

默认值为 `desktop_current_login`，保持没有旧 Router 配置的新安装和官方直连的现有
行为。该字段只引用凭据所有者，不保存任何 bearer 或 refresh token。

## UI

`OpenAI Official` Provider 卡片增加始终可见的“认证设置”操作，不再依赖 hover 才能
发现。点击后在 Provider 编辑面板的首个业务区块显示：

- `Codex Desktop 当前登录`
- `CCSM OAuth`，并可选择一个已保存账号
- `OAuth 账号池`

账号池的成员、顺序和保留额度仍在认证中心维护；Provider 面板提供明确跳转。认证中心
中的门面预览改为描述当前 `OpenAI Official` 配置，不再显示“仅影响明确选择账号池的
MultiRouter”。

MultiRouter 的“重命名/设置”保留名称、启用状态、监听地址和 hosted tools 等真正的
方案级设置，但删除“官方 ChatGPT 认证方式”。入口本身改为普通常显操作，不再只靠
悬浮出现；认证设置不再放在这里。

## 运行时

### 独立 OpenAI Official

官方 Provider 被当前 Codex Provider 选中且代理接管开启时，按
`codexOfficialAuth` 物化 effective Provider：

- Desktop：设置 native passthrough，保留可信 Codex 来向 Authorization。
- CCSM OAuth：清除来向 Authorization，绑定指定或默认的托管账号。
- 账号池：设置 `codexAccountPoolEnabled`，复用现有账号池候选展开、额度、affinity、
  冷却和失败分类逻辑。

### MultiRouter

命中 `codex-official` 的 route 后，从目标 Provider 读取当前认证配置，并物化相同的
effective Provider。修改 Provider 后下一次请求即读取新策略；route 不需要被批量
重写。

门面仍需按是否可能使用 Desktop 候选分类为 Native/Mixed 或 Fully Managed。分类器
读取目标 `OpenAI Official` Provider 和账号池策略，而不是 Router 内的复制值。门面类型
变化后重新投影 live config，并提示完全退出、重启 Codex；已有任务不宣称热加载。

## 兼容迁移

旧 `codexRouting.officialAuth` 和官方 route `authPolicy` 先保留只读兼容：

1. Provider 已有 `codexOfficialAuth` 时，以 Provider 为事实源，旧 Router 字段不覆盖它。
2. Provider 尚无新字段时，扫描所有引用 `codex-official` 的 Router。
3. 所有显式旧策略一致时，幂等写入 Provider，并清除可安全删除的重复 Router 字段。
4. 旧策略互相冲突时，不静默选择；保持各 Router 的旧行为并在 OpenAI Official 认证
   设置中展示冲突。用户保存 Provider 策略后统一转为继承。
5. 没有旧策略时写入 Desktop 默认值。

迁移不得修改账号池成员、账号顺序、额度状态、OAuth 凭据或 `auth.json`。

## 错误与安全边界

- 选择账号池但账号池未启用或没有可用候选时，保存阶段给出可操作提示，运行时明确
  返回无可用账号，不回退到 Desktop 或第三方 Provider。
- 固定 CCSM OAuth 账号不存在或需要重新认证时，不静默改用默认账号；只有未指定
  `accountId` 时才使用 OAuth 存储的默认账号。
- Desktop Authorization 只能进入显式 Desktop 或包含 Desktop 候选的账号池路径，
  不能进入托管 OAuth、第三方 Provider 或 External Agent API。
- 日志只记录认证模式和本地账号 ID，不记录 Token。

## TDD 与验收

实现前先增加稳定红灯，至少覆盖：

1. `OpenAI Official` Provider 三种认证配置的序列化、校验与保存。
2. 不存在 MultiRouter 时，账号池模式展开 Desktop/managed 候选并完成失败切换。
3. MultiRouter 官方 route 修改目标 Provider 配置后，不重写 route 即使用新认证策略。
4. Provider 认证模式改变时正确重投影 Native/Mixed 与 Fully Managed 门面。
5. 一致旧 Router 自动迁移；冲突旧 Router 保留并提示；用户显式保存后统一继承。
6. Provider 卡片认证入口常显，MultiRouter 设置中不再出现认证选择。
7. 认证中心文案和预览不再错误限定 MultiRouter。
8. Desktop bearer 不泄漏到 managed、第三方和 External Agent API。

聚焦测试通过后，再集中运行受影响的前端、Rust library、类型、格式、Clippy 和 UTF-8
门禁。除非出现跨模块失败，不在开发过程中反复运行全量测试或构建安装包。

## 非目标

- 不改变账号池优先级、保留额度、affinity、冷却和容量重试算法。
- 不把 OAuth Token 写进 Provider 或 Router。
- 不改变第三方 Provider 的认证所有权。
- 本次不发布版本、不安装新候选、不重启 Codex；发布与安装在实现和集中验收通过后另行执行。
