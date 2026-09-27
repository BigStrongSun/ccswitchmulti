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
