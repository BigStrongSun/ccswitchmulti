# 2026-08-07 Codex 生命周期守护实现

## 提交

`cfe3028e` feat(codex): 添加 Codex Desktop 模型菜单生命周期守护

## 问题回顾

用户症状：Codex 刚下载时 CCSM 可以注入模型菜单；挂代理后 Codex 开始下载更新，
更新完成后普通方式打开 Codex 不再显示 CCSM 模型菜单，必须从 CCSM 修复菜单启动
才恢复。

根因：
- `unlock_codex_model_picker()` 是一次性 CDP 注入（`Page.addScriptToEvaluateOnNewDocument`
  + `Runtime.evaluate`），绑定当前 CDP target
- renderer 内的 1.5s interval 只活在当前 document，不能跨进程/BrowserWindow 存活
- `try_repair_codex_model_picker_after_takeover()` 仅在 takeover 启用/复用时 best-effort
  调用一次，无生命周期监测

## 实现方案

新增 `src-tauri/src/codex_guardian.rs` 模块：

### 守护核心
- `start_codex_guardian()` → 返回 `GuardianHandle`
- 后台 tokio task 每 3s 执行一轮检查
- 通过 `watch::channel` 实现优雅关闭

### 检查循环
1. `detect_running_codex_main_process()` → 判断 Codex 是否运行
2. 若运行：查询 CDP target → `list_cdp_targets()` + `pick_codex_page_targets()`
3. 比较当前 target ID 与已注入列表 → 找出新增 target
4. 若有新 target → 调用 `try_inject_on_candidate_ports()` 幂等注入
5. 使用 `inject_generation` 版本号追踪注入历史
6. 清理已消失的 target ID

### 安全边界
- CDP 不可用时**绝不静默终止 Codex** → 只记录状态并等待
- 进程消失时清除已知 target → 下次启动自动检测新 target
- 重复注入通过 target ID 去重（同 ID 不重复注入）

### 平台扩展
- 检测逻辑复用 `codex_desktop` 的平台适配
  - Windows: `DETECT_CODEX_MAIN_PROCESS_SCRIPT` (PowerShell)
  - macOS: `detect_running_macos_codex_main_process()`
  - Linux: `detect_running_linux_codex_main_process()`
- `GuardianHandle` 可通过平台适配器扩展 AppX 包版本/路径监测

### 生命周期接入点
- `set_takeover_for_app("codex", true)` → `ensure_codex_guardian_started()`
- `start_with_takeover()` 成功路径 → `ensure_codex_guardian_started()`
- `set_takeover_for_app("codex", false)` → `stop_codex_guardian()`
- `ensure_codex_guardian_started()` 幂等（已有 handle 则跳过）

### 暴露的内部函数 (codex_desktop.rs → pub(crate))
- `DEFAULT_CODEX_DEBUG_PORT`, `CDP_HTTP_TIMEOUT`
- `CodexModelCatalogProjection`, `CodexAppCompatibilityEvidence`
- `CdpTarget` (字段全部 pub(crate))
- `try_inject_on_candidate_ports()`, `candidate_debug_ports()`
- `list_cdp_targets()`, `pick_codex_page_targets()`, `install_script()`
- `load_cc_switch_model_catalog_projection()`
- `detect_running_codex_main_process()`

### 前端
- `CodexGuardianStatus` 类型（`src/types/proxy.ts`）
  - `active`, `codexRunning`, `cdpAvailable`, `injectedTargetCount`
  - `injected`, `lastEvent`, `message`
- `proxyApi.getCodexGuardianStatus()` → `get_codex_guardian_status` Tauri 命令
- `CodexRouterWorkspacePage` 显示守护状态卡片（`useQuery` 每 10s 轮询）
  - 已注入：绿色边框 + 绿点
  - CDP 可用待注入：黄色边框 + 黄点
  - 轮询中/Codex 未运行：灰色边框 + 灰点
  - 接管未激活时显示提示

### 编译验证
- `cargo check` 通过
- 仅 1 个预存 `dead_code` warning（`openai_cache_read_tokens`，与本次无关）

## 修改文件
- `src-tauri/src/codex_guardian.rs` — 新建，262 行
- `src-tauri/src/codex_desktop.rs` — 暴露 pub(crate) 函数和类型
- `src-tauri/src/lib.rs` — 注册 codex_guardian 模块 + Tauri 命令
- `src-tauri/src/services/proxy.rs` — 守护字段、启停方法、生命周期接入
- `src-tauri/src/commands/proxy.rs` — get_codex_guardian_status 命令
- `src/types/proxy.ts` — CodexGuardianStatus 类型
- `src/lib/api/proxy.ts` — getCodexGuardianStatus API
- `src/components/codex/CodexRouterWorkspacePage.tsx` — 守护状态 UI

## 待验证
- 真机测试：开启 Codex 接管 → 观察守护自动注入
- 模拟 Codex 更新重启 → 观察守护自动重新注入
- 关闭接管 → 守护停止
- macOS/Linux 平台未在本机验证

## 下一步
- 可选：添加 Windows AppX 包版本监测适配器
- 可选：添加 macOS bundle 路径变更监测适配器
- 可选：添加单元测试（mock CDP target 变更）