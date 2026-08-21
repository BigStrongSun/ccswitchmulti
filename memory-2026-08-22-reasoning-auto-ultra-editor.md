# 2026-08-22 自动发现推理能力与 Ultra 编辑边界

- 自动发现是 resolver 按 Provider、模型和现有证据来源得出的运行时基线，不写入 catalog 行；因此不能把它当作用户声明或 Provider 原生档位。
- 用户需要调整映射或启用 Codex Ultra 时，应从当前解析 capability 创建 `source: "user"` 覆盖，保留已验证的档位和 `max -> Provider` 映射。Ultra 仍是 Codex V2 的产品层编排，不能作为 Provider-native effort 持久化或上游发送。
- 模型目录编辑不应依附高级选项的展开状态；高级区仅承载协议、请求覆盖和 User-Agent 等低频设置。
