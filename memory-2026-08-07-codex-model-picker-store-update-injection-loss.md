# 2026-08-07 Codex Store 更新后模型菜单注入丢失

## 用户症状

- Codex Windows App 刚安装时，CCSwitchMulti 可以注入第三方模型菜单。
- 开启代理后，Codex 的 Windows Store updater 能访问更新源并开始下载/Stage 新包。
- 更新后普通方式打开 Codex 不再显示 CCSM 模型菜单；必须从 CCSM 的“解锁模型菜单/修复”入口启动才恢复。

## 本机证据

- 当前用户注册并运行的包：`OpenAI.Codex_26.730.8199.0_x64__2p2nqsd0c76g0`。
- 2026-08-07 10:21:09 Codex 日志显示 updater 检查到 `manifestBuildVersion=26.803.5235.0`；10:21:48 返回 `canSilentlyDownload=true completed=true hasUpdate=true overallState=Completed`。
- AppXDeploymentServer 同时记录：
  - 10:21:15 将首选包从 `26.730.8199.0` 改为 `26.803.5235.0`；
  - 10:21:48 新包 Stage 成功；
  - Stage 性能摘要中的 `Registration cost: 0 ms`，说明这一步没有替当前用户完成新包注册/切换。
- 当前 Codex 主进程仍是旧包的 `app/ChatGPT.exe`，只有从 CCSM 修复入口启动后才带：
  - `--remote-debugging-port=9229`
  - `--remote-allow-origins=http://127.0.0.1:9229`
- `127.0.0.1:9229` 当前由该 `ChatGPT.exe` 监听；这解释了修复入口为什么能注入。

## 源码根因

- `src-tauri/src/codex_desktop.rs::unlock_codex_model_picker()` 是一次性动作：
  1. 尝试连接现有 CDP target；
  2. 若 Codex 未运行，则用 remote-debugging 参数启动；
  3. 对当时存在的 page target 执行 `Page.addScriptToEvaluateOnNewDocument` 和 `Runtime.evaluate`。
- 注入脚本内部的 1.5 秒 interval 只活在当前 renderer document 中；它不能跨新的主进程、BrowserWindow/webContents 或新的 CDP target 存活。
- `src-tauri/src/services/proxy.rs::try_repair_codex_model_picker_after_takeover()` 只在开启/复用 Codex takeover 时 best-effort 调用，没有常驻监听以下变化：
  - AppX 包版本/InstallLocation 切换；
  - Codex 主进程 PID/命令行变化；
  - CDP target id 新增或 renderer 重建。
- 因此 Windows Store 更新带来的包切换或普通方式重启会丢失 CDP 启动参数和 renderer 内存补丁。修复菜单重新启动并注入，只是在重建这个易失运行态。
- 记住的 Desktop 路径包含 MSIX 版本目录，更新后可能失效；现有 resolver 会回退到 AppX/manifest/WindowsApps 发现最新路径。因为修复菜单实际可以成功，路径发现不是本次主要断点。

## 修复边界

- 不能通过覆盖 `WindowsApps` 内 `app.asar` 做“永久补丁”：MSIX 包完整性保护会阻止或在下一次更新时覆盖该修改。
- 在没有 CDP 的已运行 Electron 主进程上，CCSM 不能安全地原地开启 remote debugging；要恢复注入只能等待正常退出，或在明确授权后重启一个刚启动、尚无任务的 Codex 进程。
- 根治方向应是生命周期守护，而不是继续增加一次性注入：
  1. takeover 活跃时监测 Codex 主进程、包版本与 CDP targets；
  2. CDP 可用但出现新 target 时幂等重新注入；
  3. 更新后旧进程正常退出时，从新 AppX manifest 解析当前 shell 并用 CDP 参数启动；
  4. 普通方式启动且无 CDP 时，不应静默杀掉正在工作的 Codex；需要仅拦截“刚启动且无活动任务”的短窗口，或提供显式的自动重启授权和清晰提示；
  5. 回归测试必须覆盖 package version/path 改变、PID 改变、target id 改变、重复注入幂等和运行中任务不被重启。

## 联网交叉验证

- Codex 官方 GitHub issue `openai/codex#30543` 记录 Windows App 自动下载更新以及更新中断/更新后启动异常。
- `openai/codex#30571` 汇总 Windows 安装/更新恢复路径不透明的问题。
- `openai/codex#19694` 记录 `model_catalog_json` 已被 app-server 返回但 Desktop model picker 仍过滤自定义模型，说明仅修 catalog 投影不能替代 renderer 可见性兼容层。
- Codex 内置搜索找到了上述 issue；Matrix WebSearch 独立查询可用，但本次定向 GitHub 查询未返回相关结果，不能把 Matrix 空结果当作反证。

## 当前状态

- 根因已定位并留证。
- 尚未实现 lifecycle guardian，也未做会终止/重启 Codex 的行为修改。
