# Codex MultiRouter Provider/模型单一事实源设计

日期：2026-08-20

## 目标

MultiRouter route 只描述路由策略，不再复制 Provider 连接配置或模型能力。运行时始终从目标 Provider 和 canonical 模型条目解析 effective provider；聚合模型目录是可重建投影，不是事实源。

## 所有权

- Provider：Base URL、凭据、认证绑定、默认 API 协议、请求转换、缓存和请求覆盖默认值。
- Provider 模型条目：canonical/upstream model、显示名、上下文窗口、模型级 API 协议、输入模态、推理和缓存能力。
- Route：稳定 ID/名称、启用和顺序、目标 Provider、模型选择、匹配前缀、可见别名、无密钥认证策略引用。
- Plan：默认 route、spawn-agent 用户选择、路由开关。
- Compiler 投影：可见模型目录、自动别名、精确匹配索引、模型到 route/canonical model 的解析、依赖 fingerprint。

Route v2 禁止保存 Base URL、API Key、推理/输入/缓存能力和继承协议。Provider 默认协议可由具体模型条目的 `apiFormat` 覆盖。Desktop 当前登录、托管 OAuth 和账号池只保存引用策略，绝不复制 Token。

## Schema v2

`settingsConfig.codexRouting` 增加 `schemaVersion: 2`。每条 route 使用：

```json
{
  "id": "router-qwen",
  "label": "Qwen",
  "enabled": true,
  "targetProviderId": "qwen-provider",
  "modelSelection": { "mode": "all" },
  "matchPrefixes": ["qwen"],
  "aliases": { "qwen-fast": "qwen3.8" },
  "authPolicy": { "source": "provider_config" }
}
```

`modelSelection.mode=include` 时必须提供去重后的 canonical `models`；`all` 不保存模型副本。`aliases` 的 key 是 Codex 可见模型，value 必须是当前 selection 可选的 canonical 模型。未声明 alias 时，compiler 负责稳定的冲突别名。

Provider catalog 模型增加可选 `apiFormat` 和 `codexCache`。effective API 格式优先级为模型条目 > Provider meta/settings 默认 > 保守 Chat 默认。能力优先级为模型条目用户声明 > Provider 级声明/探测/维护库 > unknown；Route 不参与能力判定。

## 运行时与投影

统一 Rust compiler 读取 plan 与目标 Provider，生成 `CompiledCodexRoutingPlan`：可见模型索引、route 引用、canonical 模型、effective 协议/能力来源摘要和 `dependencyFingerprint`。runtime、catalog 写出、诊断和迁移都调用同一 compiler。

请求按全局 exact > prefix > default 顺序选择 route。命中后重新读取目标 Provider，解析 canonical 模型并物化 effective provider；只叠加 route 的 auth policy 和别名选择。任何缓存投影 fingerprint 不匹配都必须同步重建，不能继续使用陈旧协议或能力。

投影写回 DB 后，通过现有原子文件写入器发布 Codex catalog/cache。文件发布失败标记 `projectionPending` 并暴露重试/诊断；数据库和请求运行时仍以最新 Provider 为准。

## 变更事务与删除

Provider 新增、更新、重命名、模型刷新、删除和 MultiRouter 保存进入同一后端领域服务。一次 mutation 在一个 SQLite transaction 中写 Provider、重连/级联 route、重建投影和审计记录。前端不得再计算并逐个保存关联 plan。

删除被引用 Provider 时级联移除所有引用 route。删除最后一条 route 后保留 plan，设置 `enabled=false`，清空 default route 和派生 catalog；若该 plan 正在接管，必须先用备份恢复官方配置，恢复失败则删除不提交。

## v1 迁移

v1 继续只读兼容。首次编辑、启用或显式迁移时执行 `preview -> planToken + expectedRevision -> apply`，应用必须原子、幂等并可重放预览。

- 与当前 Provider/模型一致的协议、能力和连接值转为继承并从 route 删除。
- DeepSeek 分协议迁移到对应模型条目。
- Route 内联连接/密钥、同一模型跨 route 冲突协议或能力通过克隆目标 Provider 保留；克隆 Provider 标记 `migrationGenerated=true`，route 改为引用克隆。
- 可由 catalog 重建的 modelMap 转为投影；其余转换为 `aliases`。
- 全量模型 route 转为 `mode=all`，子集转换为 canonical `include`。
- 歧义只产生 warning，不静默丢字段；迁移 diff 和日志必须脱敏。

## 验收

修改 Provider 或模型协议后，不重写 route 即影响下一次请求。混合协议、能力、OAuth、别名、精确/前缀优先级与 spawn-agent 目录保持正确。删除/重命名无悬空引用，迁移无秘密泄露，文件失败无半同步。发布前必须在受影响 Mac 上完成 Qwen Chat/Responses 双向 canary。
