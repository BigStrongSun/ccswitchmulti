# Codex MultiRouter 原子启用与 macOS ChatGPT.app 兼容设计

## 背景

CCSwitchMulti 3.19.1-20 在 macOS 新版 `/Applications/ChatGPT.app` 内嵌 Codex 环境中存在一组耦合故障：向导先用当前 Provider 开启 takeover，再切换到目标 MultiRouter；macOS Desktop discovery 假设应用和主程序都名为 `Codex`；通用 `switchProvider` 会吞掉切换异常。结果可能是官方模型目录和 legacy managed roles 先被写入、目标切换失败却由向导显示成功、运行中的 Codex app-server 没有收敛到目标配置。

这不是用户 V2 路由配置本身错误。配置和操作路径只是触发条件：新版 `ChatGPT.app`、首次从向导启用、启用前当前 Provider 不是目标 MultiRouter、尚未接管，并继续使用未重载的当前会话时最容易完整暴露。

## 目标

1. 从 MultiRouter 向导启用方案时，只使用目标 Provider 执行一次原子切换，不得先以旧 Provider 改写 live config、catalog 或 managed roles。
2. 后端切换失败必须传回向导；向导不得显示成功、关闭自身或打开成功状态页。
3. macOS 同时支持旧 `/Applications/Codex.app` 和新版 `/Applications/ChatGPT.app`，以 bundle identifier 和 bundle 元数据识别 Codex Desktop，不把文件名作为唯一身份。
4. Desktop 模型菜单/CDP 修复继续保持 best-effort，不把它错误提升为代理路由成功的必要条件，但必须保留可诊断警告。
5. Windows 和 Linux 现有 Desktop discovery、普通 Provider 切换与非向导调用方不得回归。

## 非目标

- 不改变 Sub-Agent V1/V2 schema、路由选择策略或 DeepSeek transport。
- 不把 provider、SQLite 和文件系统合并成一个无法实现的数据库事务。
- 不要求当前已打开的 Codex 对话热加载新配置；成功文案仍要求完全重启或新开会话。
- 不把所有 Desktop discovery 重写为新的跨平台框架。

## 方案选择

### 采用：复用现有后端原子入口

向导不再手动执行 `startProxyServer -> setProxyTakeoverForApp -> switchProvider`。它只发起目标 Provider 切换，使 `ProviderService::switch` 在尚未接管且目标需要本地代理时进入现有 `takeover_app_and_switch_provider_after_switch_lock`。

该入口已经负责切换锁、代理启动、live 备份、目标 current provider 设置、目标配置接管、验证及失败恢复。修复应复用并补齐这条路径，而不是在前端复制另一套事务。

### 不采用：只增加 `ChatGPT.app` 常量

这只能消除 executable warning，不能解决旧 Provider 先接管、legacy role 污染和向导假成功，也会在下一次应用改名时再次失效。

### 不采用：新增独立的一键启用后端命令

现有 Provider switch 已具备所需原子能力。新增命令会形成第二套接管事务和回滚逻辑，增加长期漂移风险。

## 详细设计

### 1. 向导启用数据流

启用流程调整为：

```text
向导保存目标 Provider
  -> 严格切换目标 Provider
  -> ProviderService 获取 app switch lock
  -> 原子启动代理并以目标 Provider 接管
  -> 验证 current provider、takeover 和 live config
  -> 返回成功
  -> 向导刷新查询并显示成功
```

向导不得在切换前调用代理启动或单应用 takeover API。普通 Provider 页面仍可保留现有交互行为。

### 2. 前端错误契约

Provider action 层提供可由调用方判断的结果。推荐把切换结果定义为判别联合：

```ts
type ProviderSwitchOutcome =
  | { ok: true; result: SwitchResult }
  | { ok: false; error: Error };
```

现有普通 UI 调用方可以继续忽略返回值并依赖 mutation toast；MultiRouter 向导必须检查 `ok`。失败时 `handleEnableCodexMultiRouterPlan` 抛出原始错误，交给向导的 `ENABLE_ERROR` 分支显示并保持窗口开启。

前置业务阻断，例如不允许的官方 Provider，也必须返回 `ok: false`，不能以普通 `return` 伪装成成功完成。

### 3. macOS Desktop discovery

macOS 以 `com.openai.codex` 作为稳定身份，并保留旧名称兼容：

- 运行态优先按 bundle identifier 定位应用，而不是只匹配进程名 `Codex`。
- 常见候选同时包含 `/Applications/Codex.app`、`~/Applications/Codex.app`、`/Applications/ChatGPT.app` 和 `~/Applications/ChatGPT.app`。
- Spotlight 主查询使用 `kMDItemCFBundleIdentifier == 'com.openai.codex'`；文件名只能作为旧版补充条件。
- 对候选 bundle 读取 `Contents/Info.plist` 的 `CFBundleIdentifier` 和 `CFBundleExecutable`，校验 bundle 身份后据此组成 `Contents/MacOS/<CFBundleExecutable>`。
- remembered executable 和反向 bundle 查找接受 `Codex.app`、`ChatGPT.app`，但仍要求 bundle identifier 正确，避免误识别独立 ChatGPT 应用。
- bundle 元数据无法读取或主程序不存在时跳过该候选并继续查找，不凭目录名强行接受。

实现优先使用 macOS 系统自带的 bundle/Info.plist 能力，不新增跨平台运行时依赖。纯路径和元数据决策应拆成可在非 macOS 测试环境运行的 helper；实际系统命令只保留在 `cfg(target_os = "macos")` 边界。

### 4. CDP 注入与路由边界

模型菜单白名单注入失败继续记录 warning，不回滚一个已经验证成功的代理接管，因为 CDP 注入不是 HTTP 路由必要条件。

原子切换成功后应继续触发现有 Codex guardian/model-picker repair 生命周期；如果当前原子入口漏掉该 best-effort 收尾，应在后端统一入口补齐，而不是让向导额外调用。

用户提示必须区分：

- MultiRouter 接管失败：启用失败，保持向导，显示真实错误。
- MultiRouter 接管成功但 Desktop 菜单注入失败：路由已启用，同时提示完全重启或新开 Codex 会话，日志保留 discovery 诊断。

### 5. managed role 收敛

修复不改变 role compiler。通过避免旧 Provider 先接管，V2 同步会直接读取目标 MultiRouter 的 `settingsConfig.codexRouting.subagentV2`。目标切换失败时由现有后端恢复 live/current provider；不得再由向导继续宣布成功。

既有 legacy fallback 仍用于确实没有 `subagentV2` 的旧 Provider，不能全局删除。

## 测试设计

### 前端

1. 向导启用调用目标 Provider switch 时，不预先调用 `startProxyServer` 或 `setProxyTakeoverForApp`。
2. 严格切换返回失败时，`ENABLE_SUCCESS` 不发生，向导保持开启并展示原始错误。
3. 普通 Provider 切换仍显示既有 toast，现有调用方忽略 outcome 时行为不变。

### Rust/macOS discovery

1. 旧 `Codex.app`、`CFBundleIdentifier=com.openai.codex`、`CFBundleExecutable=Codex` 解析成功。
2. 新 `ChatGPT.app`、相同 bundle identifier、实际 `CFBundleExecutable` 解析成功。
3. 同名 `ChatGPT.app` 但 bundle identifier 不是 `com.openai.codex` 时拒绝。
4. remembered executable 位于合法 `ChatGPT.app` 时可反查 bundle。
5. Windows `ChatGPT.exe` 仅允许 `OpenAI.Codex` MSIX 路径的现有测试继续通过。
6. Linux `Codex`/`Codex*.AppImage` 现有测试继续通过。

### 原子入口

1. 从未接管状态切换到需要本地代理的 Codex MultiRouter，current provider、takeover、live config 均指向目标。
2. takeover 写入或验证失败时恢复之前 current provider 和 live config。
3. 成功后触发 guardian/model-picker best-effort 收尾；收尾失败不把已验证路由改判为失败。

## 验收标准

- 新版 macOS `ChatGPT.app` 不再产生“只检查 Codex.app 后找不到 executable”的误报。
- 从 OpenAI Official 首次启用 V2 MultiRouter 时，不生成官方 catalog 对应的额外 legacy managed roles。
- 后端切换失败时向导明确失败且不关闭。
- 成功启用后日志和状态指向同一个目标 MultiRouter；重启或新开 Codex 会话后请求进入 `127.0.0.1:15721`。
- 前端定向测试、Rust 定向测试、TypeScript 检查、格式检查和 production build 通过。
- 至少完成 macOS x86_64/arm64 编译或 CI 构建验证；无法在当前 Windows 主机进行真实 macOS UI 验收时，必须明确记录该限制，不能宣称完成真实 Mac 运行验收。

## 发布与迁移

该修复不修改数据库 schema，不需要先卸载或清库。升级后既有 Provider、路由和用户自建 agent 文件继续保留；CCSwitchMulti-managed stale role 由目标 Provider 下一次成功同步时按现有规则收敛。

发布说明应明确建议受影响用户：退出 takeover、升级、完全退出并重启 ChatGPT/Codex，再重新启用 MultiRouter。不得要求删除整个 `~/.codex` 或应用数据库。
