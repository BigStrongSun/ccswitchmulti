# TODO: 可更新的 Provider 预设注册表

## 状态与决策

- 状态：待设计评审、待实施。当前 release 的内置预设仍可正常作为离线兜底；本 TODO 不阻塞
  当前 Provider 的模型能力编辑和保存。
- 问题：`src/config/codexProviderPresets.ts` 是编译期静态数组。用户可以修改已有 Provider 的
  `settings_config.modelCatalog`，但无法获取、比较或有选择地应用更新后的官方预设。
- 非目标：本功能不是应用二进制更新器，不下载或执行代码，不自动替换 API Key、OAuth、用户 URL、
  自定义 Header、路由、模型别名或已有用户覆盖。

## 目标

1. 内置预设保持离线可用，并作为可信的最低版本和恢复基线。
2. 不发应用 release 也能下载已验证的“数据型预设更新”。
3. 用户能够查看来源、版本、发布时间、变更摘要和逐字段 diff，并明确选择合并、仅更新未覆盖字段、
   或替换可更新字段。
4. Provider 中由用户编辑过的内容必须优先，更新不得静默覆盖凭据或运行行为。
5. 支持本地导入、导出、回滚和无网络使用。

## 数据模型

新增设备级 `preset_registry`（`~/.cc-switch/settings.json`，不进入 Provider DB）及 Provider 元数据：

```json
{
  "schemaVersion": 1,
  "sources": [{
    "id": "ccsm-official",
    "url": "https://.../manifest.json",
    "enabled": true,
    "trust": "pinned-key",
    "publicKeyId": "ccsm-preset-2026-01",
    "lastCheckedAt": 0,
    "lastAcceptedVersion": "2026.08.23"
  }],
  "cache": { "version": "2026.08.23", "expiresAt": 0, "path": "..." }
}
```

每个 Provider 的 `meta.presetBinding` 保存 `presetId`、最后应用的版本、基础快照 hash 和
用户覆盖字段集合；`settings_config` 继续是生效 Provider 配置的 SSOT。覆盖集合按模型 ID 与字段路径
记录，例如 `modelCatalog.models.deepseek-v4-flash.inputModalities`。

## 远程包与安全边界

- 第一期采用签名 manifest：schemaVersion、version、publishedAt、expiresAt、target URL、SHA-256、
  size、签名和变更摘要。客户端只接受 HTTPS、固定公钥验证、未过期且版本不回退的 manifest。
- target 仅允许声明式 Provider preset schema；拒绝脚本、任意文件路径、外部命令、环境变量注入和
  未知高风险字段。原生 catalog harness/tool policy 只能来自官方受信源，用户导入包不得携带这些字段。
- 下载写入临时文件，校验长度/hash/schema/签名后原子替换缓存；失败保留上次已验证缓存和内置兜底。
- 长期可评估采用 TUF 元数据角色；其 root/targets/snapshot/timestamp 设计可提供密钥轮换、过期和
  回滚防护。首期不能用“裸 URL + JSON”替代签名/版本/过期校验。

## 更新与合并算法

1. 获取并校验 manifest，列出可用版本但不自动应用。
2. 以 Provider 的 `presetBinding` 基础快照、远程新预设和当前生效配置做三方 diff。
3. 凭据、用户 URL、Header、路由、别名、启用状态、手工模型能力与手工推理声明默认保留。
4. 未被用户覆盖的模型展示名、上下文、输入能力、默认模型和官方 reasoning 声明可作为候选更新。
5. 同一字段同时被远程和用户修改时标为冲突，必须逐项选择“保留本地 / 采用更新”。
6. 提交前生成 plan；确认后单 DB 事务写 Provider + presetBinding，备份旧快照，并重建 live catalog。
7. 回滚只回滚该次预设应用生成的非敏感字段，不回滚之后的用户编辑。

## UI 与操作

- Provider 添加页：显示内置、已下载和本地导入预设；选择后创建 Provider 快照。
- Provider 编辑页：显示预设 ID、来源、版本、状态（最新/可更新/冲突/离线缓存），提供“检查更新”、
  “预览差异”、“应用选中项”、“恢复预设”和“断开预设绑定”。
- 设置页：管理可信源、启用/禁用、手动检查、导入 `.ccsm-preset.json`、导出脱敏预设、清除缓存。
- 不做后台静默替换；自动检查只提示可用更新。任何生效配置变更必须经过 diff 与确认。

## 实施分期与验收

### P0 - 当前可用性（已实现，需要持续回归）

- Provider 行可编辑 `inputModalities` / `supportsImage` / `textOnly`，并能恢复内置输入能力预设。
- 后端目录投影必须忠实使用显式 Provider 声明，不能仅靠模型名把用户覆盖重新降级。

### P1 - 本地可移植预设

- 版本化 schema、严格校验、脱敏导出、文件导入、diff、三方合并、事务、备份与回滚。
- 验收：导入错误文件不改变 DB；导入包不读取或输出凭据；冲突不会静默覆盖；重新启动后绑定和覆盖保持。

### P2 - 官方受签名远程源

- 固定根公钥、manifest/target 校验、缓存和过期策略、检查更新 UI、下载失败回退。
- 验收：篡改 hash/签名/过期/降级版本均拒绝；离线仍使用最后验证版本；用户覆盖在更新后保持。

### P3 - 多源与委派（可选）

- 多个可信源、优先级、组织私有签名源、密钥轮换、审计记录和策略控制。

## 测试矩阵

- Rust：schema、签名/hash/expiry/rollback、防路径穿越、事务/回滚、三方合并与覆盖保护。
- 前端：版本状态、diff、冲突逐项选择、离线/失败状态、无敏感数据展示。
- 集成：应用预设后生成 catalog、路由和子 Agent 目录；失败时 live 文件保持上个健康版本。
- 安全：伪造源、重放旧 manifest、恶意大文件、未知字段、含凭据导出包和并发编辑。
