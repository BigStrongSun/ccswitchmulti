# 2026-08-22 Codex 多版本与 CCSwitchMulti 接管审计

## 本机证据

- 当前运行的 Windows Desktop 包是 `OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0`；运行中的外壳为 `app/ChatGPT.exe`，其子进程为 `app/resources/codex.exe ... app-server`。
- Desktop 包内置的 `app/resources/codex.exe` 从二进制字符串读取到 `codex-cli 0.149.0-alpha.4.1`。因此 Desktop 当前使用的内置 Codex 运行时不是 PATH 上的 0.147.0。
- 独立安装器当前 junction `C:/Users/sunda/AppData/Local/Programs/OpenAI/Codex/bin` 指向 `~/.codex/packages/standalone/current`，包版本为 `0.147.0`；执行 `codex --version` 也返回 `codex-cli 0.147.0`。
- `~/.codex/.sandbox-bin/codex.exe` 是另一个沙箱目录中的副本，版本为 `0.148.0-alpha.9`，旁边存在多个历史 `codex-command-runner-*` 文件；它不是当前 Desktop 外壳的版本来源。
- WinGet 目录下的 `codex.cmd`/`codex.ps1` 实际是 Node.js 目录里的 npm shim，`@openai/codex/package.json` 版本同为 `0.147.0`，属于第二套独立 CLI 入口。
- `C:/Users/sunda/AppData/Local/OpenAI/Codex/bin/codex.exe` 仍是旧的 `0.137.0`，不是当前运行 Desktop 的 app-server。

## CCSwitchMulti 接管边界

- 运行中的 `cc-switch.exe` 是 `C:/Users/sunda/AppData/Local/CCSwitchMulti/cc-switch.exe`，文件版本 `3.19.2-15`，监听 `127.0.0.1:15721`，健康检查返回 200。
- `~/.cc-switch/settings.json` 的 `currentProviderCodex` 是 `codex-multirouter`，并且 `enableLocalProxy=true`。
- 当前 `~/.codex/config.toml` 的 `model_provider` 是 `codex_model_router_v2`，`base_url` 是 `http://127.0.0.1:15721/v1`，`wire_api = "responses"`。这说明接管的是 Codex 的 live 配置/请求入口，不是把某一个 CLI 可执行文件替换成另一个版本。
- 最近路由日志显示请求从 Codex 进入 `15721`，再由 `codex-multirouter::route::router-codex-official` 转到 `https://chatgpt.com/backend-api/codex/responses` 并返回 200。
- `~/.cc-switch/codex-desktop-executable.json` 记录的 Desktop 外壳路径仍是旧包 `26.818.3698.0`；当前运行进程路径 `26.818.5229.0` 更可靠，记录文件可能只是上次发现结果，不能用它判断当前 app-server 版本。

## 维护结论

- Desktop、独立 CLI、npm shim、沙箱副本分别服务不同入口；不需要为了 Desktop 额外安装多个 CLI。
- 若只用 Desktop，可保留 Desktop 包；若还用终端/IDE，再保留一套独立 CLI 即可。删除旧副本前应先确认没有脚本或 PATH 依赖，本次审计未做删除。
