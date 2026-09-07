# Pi 集成所有权与支持矩阵

## 原生事实边界

Pi 当前把自定义供应商保存在 `~/.pi/agent/models.json`，把 API Key/OAuth
凭据保存在 `auth.json`，把 `defaultProvider`、`defaultModel` 和
`defaultThinkingLevel` 保存在 `settings.json`。认证解析由 Pi 按 CLI、
`auth.json`、环境变量、`models.json` 的顺序完成。CCSwitchMulti 不复制或执行
这套认证解析。

以上结论由两条独立联网链交叉核验：Codex 内置 Web 直接读取 Pi 官方
`models.md`、`settings.md`、`providers.md`；固定入口 Matrix WebSearch 也直接
读取相同三份官方 raw 文档。两条链与本地官方提交 `84e75ad2` 的契约文档一致，
没有发现冲突。

## 支持矩阵

| Pi 资源或能力 | CCSwitchMulti 所有权 | 当前阶段 | 约束 |
| --- | --- | --- | --- |
| `models.json.providers` 显式节点 | 目标节点读写 | 核心适配器已实现 | 调用方必须提交读取时的 SHA-256 content-version；冲突拒绝 |
| `models.json` 其他顶层字段和其他 Provider | Pi/外部编辑器 | 核心适配器已实现 | 语义原样保留；每次有效写入前保存原始字节备份 |
| `auth.json` | Pi | 边界已冻结 | 不读、不写、不刷新、不删除 |
| `settings.json` 默认 Provider/Model/Thinking | Pi | 边界已冻结 | 后续只读展示；任何 Provider 操作不得改写 |
| Prompt/Skill 原生文件 | 分文件协作 | 待本 Task 后续层 | 先补文件所有权回归，再接入应用注册表 |
| Session 与 usage | Pi 会话文件只读，CCSM 用量索引自有 | 待本 Task 后续层 | 不改写 Pi session；dedup schema 与解析器一起进入 |
| Codex MultiRouter / Provider Set | Codex/CCSM | 明确不支持 | Pi 不进入其 schema 或 catalog |
| 本地代理 / failover / tray takeover | 现有应用专属 | 明确不支持 | Pi 请求仍由 Pi 原生客户端直接发出 |
| MCP sync | 现有应用专属 | 明确不支持 | 没有独立契约证据，不加入 Pi |

## 核心写入协议

1. 读取完整文件字节并计算 `sha256:<hex>` content-version；文件不存在使用
   `missing`。
2. 调用方写入或删除一个 Provider 时必须携带该 content-version。
3. 进程内所有 Pi 写入共用一把锁；锁内重新读取并比较版本。
4. 无实际内容变化时不写文件、不滚动备份。
5. 有变化时先把完整原始字节原子写入 `models.json.cc-switch.bak`，再复核一次
   原文件版本，最后原子替换 `models.json`。
6. 任何版本不一致返回 `AppError::Conflict`；不覆盖外部修改，也不触碰
   `auth.json` 或 `settings.json`。

JSONC 注释在成功写入后的规范化 JSON 中不会保留，但写前备份保留逐字节原文；
未知 JSON 值和字段在新文档中保留。结构化编辑器必须从完整 Provider 节点派生
替换值，不能只提交已知字段子集。
