# 2026-08-22 自动发现推理能力与 Ultra 编辑边界

- 自动发现是 resolver 按 Provider、模型和现有证据来源得出的运行时基线，不写入 catalog 行；因此不能把它当作用户声明或 Provider 原生档位。
- 用户需要调整映射或启用 Codex Ultra 时，应从当前解析 capability 创建 `source: "user"` 覆盖，保留已验证的档位和 `max -> Provider` 映射。Ultra 仍是 Codex V2 的产品层编排，不能作为 Provider-native effort 持久化或上游发送。
- 模型目录编辑不应依附高级选项的展开状态；高级区仅承载协议、请求覆盖和 User-Agent 等低频设置。

## 2026-08-22 Ultra 独立配置边界

- Ultra 不能绑定到 `reasoning` 的来源或 `effortMap`：前者是 Provider 能力证据，后者是 Codex 产品档位。将二者混写会迫使自动发现变成用户覆盖，破坏来源语义。
- 新持久化字段是每个 catalog 模型的 `codexUltra: { enabled, providerEffort }`。页面在统一能力卡内提供“解锁 Ultra 档”与必填的“Ultra 对应的 Provider 推理强度”；自动发现只决定下拉框可选项。
- 统一 resolver 在选出 user/detection/library/builtin/official 能力后才叠加 `codexUltra`。它保留原 capability 来源，用用户选择的 `providerEffort` 建立内部 `max` 出站路径，最终才向 Codex 暴露 Ultra。未选择或超出已确认能力范围的配置 fail closed。目录、请求、Sub-Agent 都复用该 resolver。

## 2026-08-22 Provider 模型目录行的 Ultra 入口

- 已按确认的 Provider 页面顺序实现：每个模型目录行从左至右依次显示模型身份、推理能力摘要、独立的 Ultra 解锁与 Provider 强度选择、配置按钮。小屏保持同一顺序纵向排列。
- Ultra 控件不再藏在“配置推理能力”展开卡中。能力来源、自动发现说明、映射和上游协议细节仍留在展开配置，避免把产品编排设置误当作 Provider 能力来源。
- 模型行从 resolver 当前结果取得 `providerAcceptedEfforts`，因此无论能力来自自动发现、维护库、用户声明或官方目录，Ultra 下拉都只提供上游实际接受的强度；开启但未选择强度仍由 Provider 保存门禁拒绝。
- 回归：`pnpm typecheck`、模型摘要/Provider 表单/编辑器 46 条定向 Vitest、Rust `catalog_ultra_setting_overlays_library_capability_without_changing_its_source`、`git diff --check` 均通过。定向前端测试仍会输出既有 React `act(...)` 警告，但退出码为 0。

## 2026-08-22 本地 Windows 构建证据

- 源码提交 `75f2149` 已通过仓库标准 `pnpm release:export` 流程构建；没有执行安装、停止服务或变更现有运行态。
- 导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，包含 `CCSwitchMulti_3.19.2-15_x64-setup.exe`、便携 ZIP、原始 EXE、NSIS installed-exe 哈希、`latest.json` 与 SHA-256 清单。
- 安装包 SHA-256 为 `7B707FEE7E41D7D47F08E3A57217C917EAAA2D611F24D1FFE9FB99EE46411676`；`.sig` 存在。15 个清单条目全部重新计算匹配；EXE FileVersion/ProductVersion 均为 `3.19.2-15`，`latest.json` 仅包含本机可构建的 `windows-x86_64` 更新条目。
