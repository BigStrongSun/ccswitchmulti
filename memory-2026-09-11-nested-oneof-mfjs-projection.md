# 2026-09-11 DeepSeek MFJS 嵌套 oneOf 根修

- 运行时 3.20.2-3 日志再次出现 `automation_update` 本地 422，路径固定在 `$.tools[11].tools[0].parameters.anyOf[1].oneOf`。这不是 DeepSeek 上游拒绝，也不是旧版的“DeepSeek 误判 MFJS”回归；Provider 的 Responses probe 已显式留下 `ambiguous_rejection -> moonshot_mfjs -> verified` 证据，运行时按证据选 MFJS，但 MFJS 编译器还没准备好 Codex 最新 automation schema。
- 最新 schema 把一个纯 `oneOf` 嵌进根 union 的某个分支里。旧流程先递归编译子 schema，编译器在嵌套分支处按“oneOf 必须可证明互斥才能变 anyOf”提前失败；原有的“根 union 投影成普通 object”此时还没执行。不能把重叠 oneOf 静默改成 anyOf，因为这会破坏独占语义。
- 根修是只在根 union 预处理阶段递归展开“纯 union 分支”（对象只含一个 oneOf/anyOf 键），把可调用 object 分支提升到同一层；随后保留原来的 MFJS 编译和根 object union 投影，并照旧关闭 strict。带额外约束的 union 分支不会被展开，避免丢弃约束或误改语义。
- TDD 回归 `moonshot_schema_projects_nested_root_union_branches_for_codex_dynamic_tools` 先稳定复现同路径 RED，再转 GREEN。最终 Rust library 4091 passed / 0 failed / 7 ignored，`moonshot_schema` 19/19、`codex_request_tests` 27/27、`cargo check --all-targets`、rustfmt 通过。一次并行全量出现既有 `codex_config_consistency` CAS 并行扰动，单独重跑通过；这属于测试隔离已知问题，不归因本次改动。
- 源码修复必须重建并替换安装态后才能声称用户界面/代理已修复；当前安装态 `C:/Users/sunda/AppData/Local/CCSwitchMulti/cc-switch.exe` 仍是 3.20.2-3，源码修复提交前不能算 runtime shipped。

## 2026-09-11 二次根修：根 union 分支经 `$ref` 间接指向嵌套 union

- 安装 `b53b1aa0` 后同一 422 在 16:21 仍复现，先排除安装态漂移：15721 listener、PID/启动时间、安装文件 SHA-256 均为新 `3.20.2-3`。因此不是“替换没替换成”，而是首修的展开条件不完整。
- 对照 Codex 工具 schema 发现真实根 union 分支不是内联 `oneOf`，而是 `{"$ref":"#/$defs/..."}`；目标 definition 才是纯 union。首修只展开内联纯 union，因此在根 projection 前没有命中。
- 新增 TDD 回归模拟这种根分支 `$ref -> 纯 union` 结构，先复现同路径 RED。根修改为在根 union 预处理时按 `$defs` 解析仅含 `$ref` 的分支，再递归展开纯 union；循环引用保留给普通编译器报错，普通属性 `$ref` 不提前展开。
- 二次验证：新回归 GREEN，Rust library 4096 passed / 0 failed / 7 ignored（其中既有 CAS 并行扰动单独重跑通过），聚焦回归通过，`cargo check --all-targets` 和 rustfmt 通过。需再次本地 release + 事务安装后才能验证当前代理。
