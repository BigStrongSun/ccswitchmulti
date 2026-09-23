# TE Provider 原子注册候选与隔离链路（2026-09-23）

## 边界

- 产品源码候选：`bigstrongsun/te-provider-main-integration@8661486d94af868b402256086b984e4acf880cf0`，版本 `3.20.2-19`。构建期间 tracked 工作树 clean，既有 `.tmp/` 未纳入产物。
- 只构建独立 Windows x64 候选，不安装、不替换、不重启当前 CCSM/OpenClaw；不推送、不签发正式发布版本。
- Cargo Release target 是 `D:\ccsm-te-descriptor-release-20260923-8661486d`，与 debug 测试和其他代理构建隔离；C 盘空间不足时没有创建新的共享 target。

## 候选产物

先用同一独立 target 构建 `codex-history-repairer --features history-repairer --release`，再执行 `pnpm tauri build --bundles nsis --no-sign --config '{"bundle":{"createUpdaterArtifacts":false}}' --ci`；renderer、Release 主程序和 NSIS 均退出 0。

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| `D:\ccsm-te-descriptor-release-20260923-8661486d\release\cc-switch.exe` | 44,440,576 | `6291CEC59A194591A30A07D8DE8FA5FB1E3780C55F97FEFC708E8B5C88AC006A` |
| `D:\ccsm-te-descriptor-release-20260923-8661486d\release\codex-history-repairer.exe` | 2,301,440 | `7E26A3C0E335B0C8F13419E73CFD5E809130C2964886C8AA036D1ED15C28AC8D` |
| `D:\ccsm-te-descriptor-release-20260923-8661486d\release\bundle\nsis\CCSwitchMulti_3.20.2-19_x64-setup.exe` | 13,816,068 | `CB7D561694A984FC72BD3FF2B8FBF31D50614A3DBE6550FFD0CE3B6E9EFB241F` |

raw EXE 的 Windows FileVersion / ProductVersion 均为 `3.20.2-19`。NSIS 的 Authenticode 状态是 `NotSigned`；没有声称 updater 签名、已安装二进制或在线进程与候选一致。

## 同批 companion 契约

本地归档 `token_exchange/platform/clients/ccsm-te-provider-sdk/dist/managed-host/2026-09-23-cp312-linux-x86_64-77a4f773.tar.gz` 的 SHA-256 为 `30234D0D85B6CEFD73D1D252431AC703F6356CD48313D3160FD12904C3754011`；`manifest.sha256` 为 `A5265262691A604589E4E1CEF61F911DE1B410C3C8303FF820089EF0BA330E16`，58 项逐一重算一致。

归档 wheel metadata 为 `ccsm-te-provider-sdk 0.1.0`、`token-exchange-sdk 0.2.2`；OpenClaw plugin `2026.6.11` 声明兼容 `>=2026.6.11 <2027.0.0`。生成器 `TeProviderConfig` 对同一个 `te-qa` 配置分别输出严格静态 descriptor 与 `{api, baseUrl, apiKey, models}` 公网投影，与 `save_te_provider` 的双对象核对一致。旧 companion README 的独立 `registry.upsert` 示例与新原子 IPC 冲突，交由 companion 负责人修订；不能把文档示例当作已连接入口。

## 隔离服务与真实 loopback sidecar

从归档把两个纯 Python wheel 安装进 `D:\ccsm-te-companion-qa-20260923-8661486d\venv`，仅供 Windows 隔离验证；其 FastAPI/Uvicorn 来自该 Python 环境，不等同完整 Linux CPython 3.12 归档部署。由该环境在 `127.0.0.1:29814` 启动无活跃 Task 的实际 `create_loopback_sidecar_app(TeProviderRuntime(...))`，随后显式设置 `CCSM_TE_QA_SIDECAR_URL=http://127.0.0.1:29814` 执行被默认忽略的 `te_provider_isolated_live_projection_reaches_companion_sidecar`：1/1 通过。

此测试以 `with_test_home` 隔离 `CC_SWITCH_TEST_HOME`/`HOME` 和内存 SQLite，执行 `save_token_exchange_openclaw(..., add_to_live=true)`，确认返回 `stored_and_published`；私有 descriptor 与公开 Provider 成对保存，隔离 `~/.openclaw/openclaw.json` 写入 baseUrl `/v1` 与公开占位 key，不含 `teProvider`。运行态 GET `/healthz` 返回 `200 {"status":"ok"}`，provider 在线状态仍为未知；未授权的真实 POST `/v1/chat/completions` 返回 `428 te_proxy_key_required`。带 `proxyKey` 的后续更新被拒且 DB/live 投影保持原值。既有 DAO 触发器测试另覆盖 descriptor INSERT/UPDATE 失败时 SQLite 事务回滚。

测试运行时设置 `CARGO_TARGET_DIR=C:\Users\sunda\AppData\Local\Temp\ccsm-te-descriptor-target-20260923` 及 `CCSM_TE_QA_SIDECAR_URL=http://127.0.0.1:29814`，执行 `cargo test --manifest-path src-tauri/Cargo.toml --lib te_provider_isolated_live_projection_reaches_companion_sidecar -- --ignored --test-threads=1 --nocapture`，结果 1/1；未提供端点且不用 `--ignored` 时结果是 0 passed、1 ignored，不会空跑通过。验收后只停止本次启动的 sidecar，端口 29814 已无 listener。

**尚未证明**：桌面窗口实际点击 Add 的 Tauri IPC 跨进程路径、插件在目标 OpenClaw 版本上的加载、经认证 AIP Task Start/bind/revoke、真实 TE Proxy 调用、正式安装与在线切换。这些必须在同一隔离宿主具备可信身份和任务材料后再验收；本候选不能作为生产放行结论。
