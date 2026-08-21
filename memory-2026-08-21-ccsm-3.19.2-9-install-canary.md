# CCSwitchMulti 3.19.2-9 安装态 canary（2026-08-21）

## 结果

- 事务安装脚本已安全替换已安装实例；监听端口 `127.0.0.1:15721` 的进程已从旧安装态切换到新安装态。
- 安装路径：`C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe`。
- 版本：`3.19.2-9`。
- 安装后二进制 SHA-256：`C6768833CF83FABD12EBD9A308A466490EE95DB9889D5DB61B8C504EFA12D92A`。
- 事务日志：`C:\Users\sunda\AppData\Local\CCSwitchMultiTransactionBackups\ccsm-20260821-3.19.2-9-115654\`，事件为 `preflight-ok`、`backup-ok`、`transaction-success`，新 PID 为 `24240`。
- `http://127.0.0.1:15721/health` 返回 `200` 且 `status=healthy`。

## 发布阻塞点

- 当前仓库 HEAD 是 `7a9189dc`，而本地 `v3.19.2-9` 标签指向旧提交 `d014a24e`。
- 因此不能把当前 HEAD 直接作为同名 `v3.19.2-9` 发布；发布前必须确定新的版本号（例如 `3.19.2-10`），同步 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 与发布元数据，再重新构建并验证各平台产物。
- 在版本号和跨平台产物未完成前，不应 push、移动现有标签或创建 GitHub Release。
