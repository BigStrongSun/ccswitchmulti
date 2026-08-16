# 2026-08-07 Codex 模型菜单注入的 macOS/Linux 边界

## 结论

- “CDP/renderer 注入绑定当前进程与 target，进程重启或新 renderer target 会丢失”是跨平台问题，不是 Windows 专属。
- “代理可用后 Windows Store updater 下载并 Stage 新 MSIX，包版本/注册状态切换”是 Windows 特有触发链，不能套到 macOS/Linux。
- OpenAI 官方 Desktop 文档在 2026-08-07 只提供 Windows 和 macOS 下载，没有 Linux Desktop。Linux 官方支持边界应按 Codex CLI/app-server 处理，不能把 CCSM 源码里的 AppImage 兼容探测描述成官方 Linux Codex Desktop 支持。

## macOS

- CCSM 当前可从运行中 `Codex` 进程、`/Applications/Codex.app`、`~/Applications/Codex.app` 和 Spotlight 查找 Desktop shell，目标二进制是 `Codex.app/Contents/MacOS/Codex`。
- 启动时通过 `open <Codex.app> --args --remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229` 传入 CDP 参数。
- `unlock_codex_model_picker()`、`Page.addScriptToEvaluateOnNewDocument`、当前 document 内 1.5 秒 interval 和 takeover 后的一次性 best-effort 调用都与 Windows 共用，因此：
  - Codex.app 更新后重启且没有保留启动参数时，注入会丢失；
  - 主进程不变但创建新的 BrowserWindow/webContents/CDP target 时，旧 target 的注册和脚本也不会自动覆盖新 target；
  - Finder/Dock 普通启动一个没有 CDP 的 Codex 时，CCSM 无法在运行后安全地原地开启 remote debugging。
- macOS bundle 路径通常仍是 `/Applications/Codex.app`，所以不像 Windows MSIX 那样天然包含版本目录；路径漂移风险较低，但注入生命周期问题仍在。
- 本轮没有 macOS 真机和更新日志，以上是官方平台可用性与当前 CCSM 源码边界结论，不是 macOS 更新流程的现场复现。OpenAI 官方文档未在本轮检索中给出桌面 updater 的具体重启/参数保留实现，因此该细节证据不足。

## Linux

- OpenAI 官方 Desktop 文档当前没有 Linux 下载；Linux 正常产品边界是 Codex CLI/app-server，没有 Electron renderer 模型菜单可注入。
- CCSM 源码保守兼容大写 `Codex`、`Codex.AppImage`、PATH、绝对路径 `.desktop Exec`、`/opt`、`~/.local/bin` 和 `~/Applications`，并明确排除小写 `codex` CLI。
- 该代码只能支持用户自行安装的兼容 Electron shell/非官方打包；如果此类 AppImage 或 shell 重启、替换路径或创建新 target，同样会丢失一次性注入。
- `.desktop` 解析目前只接受绝对路径 Exec，并忽略 Flatpak/Snap 包装命令；不能据此声称 Flatpak/Snap Codex Desktop 可用。
- Linux 不应把“模型菜单守护”作为默认官方路径；应继续以 live `config.toml`、`model_catalog_json`、本地 `/v1/models` 和 MultiRouter 转发验证 CLI/app-server。

## 后续实现约束

- 生命周期 guardian 的公共核心可以跨平台复用：跟踪主进程 identity/PID、CDP 是否可用、renderer target id 和已注入版本。
- 平台适配层应拆开：
  - Windows：AppX package family、版本与 InstallLocation；
  - macOS：bundle URL/可执行文件、PID，以及 `open --args` 重启；
  - Linux：仅在发现明确兼容 Desktop shell 时启用，记录 AppImage/可执行文件路径和 PID。
- CDP 仍可用且 target 改变时可幂等重注入；进程已无 CDP 时不能原地修复，必须等待正常退出、走受控启动，或获得明确的重启授权。

## 检索交叉验证

- OpenAI 官方 Desktop 文档实际打开后写明下载 Windows 或 macOS，未列 Linux。
- Matrix WebSearch 独立调用可用，但本次查询没有返回可用于确认官方 Desktop 平台或 updater 实现的一手结果。
- 因此平台可用性以官方文档为准；CCSM 行为以本地源码为准；macOS/Linux 更新后是否复现仍需对应真机验收。
