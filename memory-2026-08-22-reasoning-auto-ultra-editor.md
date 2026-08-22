# 2026-08-22 自动发现推理能力与 Ultra 编辑边界

- 自动发现是 resolver 按 Provider、模型和现有证据来源得出的运行时基线，不写入 catalog 行；因此不能把它当作用户声明或 Provider 原生档位。
- 用户需要调整映射或启用 Codex Ultra 时，应从当前解析 capability 创建 `source: "user"` 覆盖，保留已验证的档位和 `max -> Provider` 映射。Ultra 仍是 Codex V2 的产品层编排，不能作为 Provider-native effort 持久化或上游发送。
- 模型目录编辑不应依附高级选项的展开状态；高级区仅承载协议、请求覆盖和 User-Agent 等低频设置。

## 2026-08-22 Ultra 独立配置边界

- Ultra 不能绑定到 `reasoning` 的来源或 `effortMap`：前者是 Provider 能力证据，后者是 Codex 产品档位。将二者混写会迫使自动发现变成用户覆盖，破坏来源语义。
- 新持久化字段是每个 catalog 模型的 `codexUltra: { enabled, providerEffort }`。页面在统一能力卡内提供“解锁 Ultra 档”与必填的“Ultra 对应的 Provider 推理强度”；自动发现只决定下拉框可选项。
- 统一 resolver 在选出 user/detection/library/builtin/official 能力后才叠加 `codexUltra`。它保留原 capability 来源，用用户选择的 `providerEffort` 建立内部 `max` 出站路径，最终才向 Codex 暴露 Ultra。未选择或超出已确认能力范围的配置 fail closed。目录、请求、Sub-Agent 都复用该 resolver。
