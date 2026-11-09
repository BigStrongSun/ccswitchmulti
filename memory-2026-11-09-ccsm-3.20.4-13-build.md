# 2026-11-09 CCSwitchMulti v3.20.4-13 集成与 Windows 本地构建

## 源码与版本边界

- 权威仓库：`C:\Users\sunda\Documents\LLMservice\cc-switch`，分支 `main`。
- 生图 API 修复：`ba7d08f6c`，为 `gpt-image-*` / `chatgpt-image-*` 增加仅命中 OpenAI Official 的 schema-v2 回退。
- 账号池 503 修复：`913d72515`，拆分额度尝试时间与成功观测时间；失败刷新不再延长旧额度新鲜度。
- 发布元数据提交：`f0e908d53`，产品版本从已发布的 `3.20.4-12` 升到 `3.20.4-13`，避免同一版本号对应不同二进制。
- 构建使用冻结提交 `f0e908d531e5f15839c11066979e39b11e6df7e6`，干净 detached worktree，来源元数据记录在产物根目录 `RELEASE-METADATA.md`。

## 回归与构建

- `v2_image_generation_routes_to_official_identity_without_catalog_entry`：1/1 通过。
- `proxy::providers::codex_oauth_pool::tests`：10/10 通过。
- `--lib pool`：36/36 通过，覆盖 pool manager、forwarder 与路由交互。
- `pnpm typecheck`：通过。
- 本地发布流水线从干净提交完成：`pnpm install --frozen-lockfile --force`、隔离 Cargo target、sidecar、前端、Tauri release、NSIS 全部退出 0。
- 构建开始时 WSL Git worktree 的 `.git` 与父 `gitdir` 使用 `/mnt/c/...`，Windows Git 报 `not a git repository: (NULL)`。已将这两个 worktree 元数据改为 Windows 绝对路径后成功构建；这不是产品源码问题。

## 产物

- 根目录：`C:\Users\sunda\Documents\LLMservice\cc-switch\.local-archives\releases\v3.20.4-13`
- NSIS：`windows\installer\CCSwitchMulti_3.20.4-13_x64-setup.exe`
- NSIS SHA-256：`B38F766774A9DF3E87629F9F383473E6FE06F1344BF964D9BB254A8233ADA6AB`
- raw EXE：`windows\raw-exe\CCSwitchMulti_3.20.4-13_x64.exe`
- raw EXE SHA-256：`67A86EA0136D484CF5C908F42796815A409B5B901C20BD940EA2625AE795E036`
- portable ZIP SHA-256：`F0666745049DCE9A43F899C6FED6D2E2524ED4B4AB7926FB31FABE18C5707113`
- updater signature SHA-256：`22BE89D016CC2B0064FD62C54F046FE389FA984FD8DBD0C58628CE3A6BA78DCC`
- raw EXE 与 NSIS 的 `ProductVersion`、`FileVersion` 均为 `3.20.4-13`；文件长度分别为 44,933,120 和 14,064,292 bytes。

## 远程同步

- 推送前 `fork/main` 为 `181a16e25`，本地 `main` 为其纯快进后继。
- `git push fork main` 成功，远端 ref 与 GitHub API 均确认 `fork/main = a2f0540895279310a0a25200bb91e3c55a93afd3`，领先原远端 4 个提交。
- 本次没有创建或推送 `v3.20.4-13` tag，也没有触发 GitHub Release workflow。

## 未完成边界

- 没有替换或重启现役安装 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe`；现场仍是 `3.20.4-11`。
- 没有发送真实付费 ImageGen 请求，也没有完成真实 Codex 客户端在账号池恢复后的 503 在线回归。
- 本地 Windows 构建只证明提交 `f0e908d53` 可打包并通过受控回归；不代表 GitHub Release workflow、macOS/Linux、安装事务或真实上游验收。
