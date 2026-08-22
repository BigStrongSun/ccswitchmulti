# 2026-08-22 自动发现推理能力与 Ultra 编辑边界

- 自动发现是 resolver 按 Provider、模型和现有证据来源得出的运行时基线，不写入 catalog 行；因此不能把它当作用户声明或 Provider 原生档位。
- 用户需要调整映射或启用 Codex Ultra 时，应从当前解析 capability 创建 `source: "user"` 覆盖，保留已验证的档位和 `max -> Provider` 映射。Ultra 仍是 Codex V2 的产品层编排，不能作为 Provider-native effort 持久化或上游发送。
- 模型目录编辑不应依附高级选项的展开状态；高级区仅承载协议、请求覆盖和 User-Agent 等低频设置。

## 2026-08-22 自动发现 Ultra 入口修复

- `3.19.2-15` 的源码虽有 Ultra checkbox，但它藏在“配置推理能力 → 按当前结果自定义 → Codex Ultra 编排”之后，且要求用户先补 `max` 映射；自动发现状态没有直接可见的 Ultra 操作，违背了独立开关的产品要求。
- 修复将“开启 Ultra”直接放到自动发现结果卡片。点击时只在模型已经确认分档推理能力的前提下创建 `source: user` 覆盖，补全恒等映射，并将 `Codex max` 映射到已确认的最高 Provider effort（`max > xhigh > high > medium > low > minimal`），再持久化 `codexUltraOrchestration.enabled=true`。例如自动发现 `low/high` 的 DeepSeek 会得到 `max -> high`。
- 未确认分档能力不猜测或伪造 Ultra：按钮路径返回可操作错误，用户必须先手动声明实际支持的档位。回归覆盖直接启用、映射选择和配置校验；`pnpm typecheck`、相关 41 个前端测试与 `git diff --check` 通过。
