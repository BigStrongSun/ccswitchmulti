# 2026-09-27 官方模型目录刷新与档位生效边界

## 根因与证据

- CCSM 的同步配置投影为了避免接管锁内网络卡顿，后台调度公共官方目录刷新；旧后台路径只保存 `codex-official-models-cache.json`，没有重建派生的 catalog/cache，因此能力更新要等另一次投影。
- 当前 Codex 配置使用 `model_catalog_json`。OpenAI 官方 `models-manager/src/manager.rs` 的 StaticModelsManager 保留内存目录；公开 issue https://github.com/openai/codex/issues/35129 复现同进程改文件及 reloadUserConfig 后 model/list 不变。
- 前轮独立 app-server 枚举已返回 GPT-6 Sol/Luna 完整 reasoning/speed 字段；不能据此断言当前桌面进程已读到它们。文件 LastWriteTime 也不能证明首次写入时间。

## 修复边界

- 公共刷新保存成功后比较完整模型内容，忽略条目顺序；变化时在网络预算之外调度 catalog-only 重投影。
- 使用应用启动时注册的 AppHandle，所有公共刷新入口共用成功后处理，不只处理开启代理入口。
- 重投影使用现有 Codex switch lock，重新检查接管状态、当前 Router 和目录所有权，读取当前 DB provider；不重写用户认证、路由或 TOML。
- 这修复 CCSM 自动投影缺口，不承诺运行中的 Codex 静态模型管理器热加载。未直接获取旧桌面进程的当时 model/list，UI 根因仍有此证据边界。

## 验证状态

- `cargo test --lib official_catalog`：8 passed，0 failed；涵盖实际公共快照存储、变化识别及接管关闭时拒绝自动投影。
- `cargo check --manifest-path src-tauri/Cargo.toml --lib --no-default-features`：exit 0；rustfmt、diff 检查通过。
- 尚未验证自动网络刷新到实际生成目录的成功链路，未做完整测试套件、构建、安装、重启或 UI 验收；不能宣称用户控件问题已解决。

## 本地 Windows 构建

- 2026-09-27 13:18:51，正式 local-release-pipeline 完成；版本 `3.20.2-22`，源码 `0d7c75265067e182023998a4e119eeca06b5d1e4`（包含 `c4835de21`）。TypeScript 检查、release 编译及 NSIS 打包通过。
- 产物在 `C:/Users/sunda/Documents/LLMservice/最新版ccswitchmulti`，包含 Windows installer、portable 与 SHA256SUMS。构建缓存自动清理约 3.2 GiB。
- 未安装、未重启、未发布 GitHub；完整自动刷新成功链路及控件 UI 验收仍待验证。

## 2026-09-29 安装态只读核验

- 已安装主程序 `C:/Users/sunda/AppData/Local/CCSwitchMulti/cc-switch.exe` 的 FileVersion 为 `3.20.2-22`；SHA-256 等于本地发布目录 `windows/installer/CCSwitchMulti_3.20.2-22_x64-installed-exe.sha256`。PID 33860 于 13:20:20 启动，持有 `127.0.0.1:15721`，`/health` HTTP 200。
- 当前 Codex app-server PID 21088 于 13:30:10 启动；CCSM 生成目录与 `models_cache.json` 的 LastWriteTime 均为 13:20:21，且 GPT-6 Sol/Luna 条目含推理档位、Fast 与 priority 元数据。这符合新进程读取目录的时间顺序，但并非当前进程 `model/list` 或 UI 控件的直接证明。
- Windows HKCU 卸载项 `DisplayVersion` 仍为 `3.20.2-18`，与已安装/运行文件不一致；本轮只读验收未改注册表。
- 未触发官方目录强制刷新、未重启 Codex 或 CCSM，自动刷新成功链路与 UI 档位控件仍未完成端到端验收。
