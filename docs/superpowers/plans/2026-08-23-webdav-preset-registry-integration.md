# 联调设计：WebDAV 同步 × 可更新 Provider 预设注册表

## 背景与结论

本设计回答三个问题：

1. **当前 WebDAV 同步是否有问题？**
   协议本身（v2，整库快照 + last-write-wins + 无合并）在本机未配置（`~/.cc-switch/settings.json`
   无 `webdav_sync` 键），代码路径完整可用。已知限制是**多设备并发冲突只上报、不仲裁**
   （见 `memory.md` 开放问题）。这不是 bug，是首版设计取舍。真正的隐患见第 3 点：
   预设绑定元数据若按原 TODO 放在 `settings.json`，WebDAV 同步**不会**带上它，导致跨设备
   覆盖保护失效。

2. **最近做的“远程预设表”是什么？**
   即 `docs/superpowers/plans/2026-08-23-remote-provider-preset-registry-todo.md`：把
   `src/config/codexProviderPresets.ts` 的编译期静态数组，升级为可获取、可比较、可选择性应用
   的“数据型预设更新”。P0（可编辑 `inputModalities`/`supportsImage`/`textOnly` + 后端投影回归）
   已实现；P1（本地可移植预设 + 三方合并）、P2（签名远程源）、P3（多源）尚未实现。

3. **两套能否联调？**
   **能，且边界清晰。** 二者是不同平面，不是重复：
   - WebDAV 同步 = 用户**自有**多设备**状态**同步（整库、LWW、无签名）。
   - 预设注册表 = **官方预设数据**分发（版本化、签名、三方合并、用户覆盖保护）。

   联调的两个落点：
   - **传输复用**：预设注册表 P2 的远程源可以直接跑在用户已有的 WebDAV/S3 上，复用
     `services/webdav.rs` 的 `get_bytes`/`head_etag`/`ensure_remote_directories`/`put_bytes`
     原语，无需官方 HTTPS 端点。
   - **一致性**：`presetBinding` 必须落在 **DB（`providers.meta`）**，而不是 `settings.json`，
     这样 WebDAV 整库同步会自然带上它，跨设备覆盖保护才成立。

## 关键设计决策

### D1：`presetBinding` 落在 `providers.meta`（DB），不进 `settings.json`

- 原 TODO 把 `preset_registry`（设备级源列表 + 缓存）放 `settings.json`，把 `presetBinding`
  放 Provider 元数据。问题：WebDAV 同步只上传 `db.sql` + `skills.zip`，**不上传
  `settings.json`**。若 `presetBinding` 在 `settings.json`，设备 A 应用预设后，`modelCatalog`
  变更（DB）会同步，但 `presetBinding`（哪个字段来自预设、基础快照 hash、用户覆盖集合）
  不会同步 → 设备 B 丢失覆盖保护，未来更新可能静默覆盖用户编辑。
- **决策**：`presetBinding` 写入 `providers.meta`（该列已存在，`TEXT NOT NULL DEFAULT '{}'`，
  且 `providers` 在同步触发表内、不在 `SYNC_SKIP_TABLES`/`SYNC_PRESERVE_TABLES` 内），
  随整库同步自然跨设备一致。
- 设备级 `preset_registry`（源列表 + 缓存）仍放 `settings.json`：它是**本机**如何获取预设的
  配置（含 WebDAV 凭据），属于设备私有，不应跨设备同步（凭据尤其不能同步）。

### D2：WebDAV/S3 作为预设注册表 P2 传输

- 复用 `services/webdav.rs` 原语。预设源布局：
  `{base_url}/{remote_root}/presets/{profile}/manifest.json`（与同步布局
  `{remote_root}/v2/db-v6/{profile}/` 平行，互不干扰）。
- WebDAV 本身**不提供签名**。因此 WebDAV 预设源必须携带**离线签名**的 manifest：
  用受信私钥在发布端签名，客户端用固定公钥验证。WebDAV 只负责“把签名好的文件传过来”。
- 这满足 TODO 的安全红线：**没有受信源 + 签名验证前，不得做裸 URL 下载更新。**

### D3：信任分层

- `trust = "pinned-key"`：固定公钥 + Ed25519 签名 + SHA-256 + 过期 + 版本不回退，全部通过才接受。
- `trust = "local"`：仅本地导入 / 用户显式信任的源，跳过签名但保留 hash/过期/版本校验，
  UI 明确标注“未签名”。
- 内置预设（`codexProviderPresets.ts`）永远是离线兜底与最低版本基线，远程更新失败时回退。

### D4：整库同步与版本化预设合并如何共存

- 二者作用在不同字段集，不冲突：
  - WebDAV 整库同步搬运**全部** `providers.settings_config`（含 `modelCatalog`）与 `providers.meta`
    （含 `presetBinding`），是“状态搬运”。
  - 预设更新是“**有选择地**改写 `settings_config` 中未被用户覆盖的字段”，并更新 `presetBinding`
    的覆盖集合与基础快照 hash。
- 顺序约定：预设更新在**本地**产生一次 DB 写（单事务：Provider + presetBinding + 备份旧快照），
  随后由既有的 auto-sync 触发器（`providers` 表变更）把结果同步出去。即：**预设更新是同步的
  输入，不是同步的替代**。
- 冲突仲裁仍沿用整库同步的 LWW（首版）；预设三方合并只在“同一设备本地应用预设”时发生，
  不跨设备仲裁。跨设备并发编辑同一 Provider 的既有 LWW 限制不变（见 D5）。

### D5：已知限制（不在本次联调范围，显式记录）

- 多设备并发编辑同一 Provider 仍是 LWW，无字段级仲裁。预设覆盖保护只在“本地应用预设”时生效。
- 预设注册表 P1 的三方合并 / diff / UI、P2 的缓存与过期策略、检查更新 UI 为后续工作；
  本次联调交付**传输 + 配置 + manifest 校验**这一可验证地基。

## 治理与维护模型（谁维护 / 条目怎么进 / 审核）

联调方案只解决了“完整性”（签名保证 manifest 来自受信发布方且未被篡改），没解决“治理”
（目录归谁、条目怎么进、变更要不要审）。这一节补齐。

### 现状：已有基础设施，直接复用（不新建）

- 已有 Cloudflare R2 桶 `cc-switch-releases` + `.github/workflows/sync-r2.yml`：release 晋升
  （`released` 事件，**非** tag push）时把 release 资产 + Tauri updater manifest 镜像到 R2
  （`dl.ccswitch.io`）。
- updater 客户端用 **minisign** 验签；R2 镜像被显式当作**不可信**——信任来自签名，不来自传输。
- 这套“不可信镜像 + 客户端验签 + release 晋升门控”正是预设目录需要的治理模型，直接复用。

### G1：目录单独维护，但不新建基础设施

- 目录源放 git（先放主仓库；要社区贡献再拆独立 `ccsm-model-catalog` 仓库）。
- 内置 `codexProviderPresets.ts`（2021 行，29 个 modelCatalog）继续当离线兜底，现状不变。
- 发布步骤：读目录源 → 校验（schema / 禁字段 / 无凭据 / 无可执行配置）→ 生成 manifest →
  签名 → 传 R2/WebDAV。复用现有 R2 + minisign + release 门控，不另起炉灶。

### G2：审核机制 = 你已有的 release 晋升门控 + PR 评审

- 目录变更走 PR（人工评审 diff——这一步抓“合法但错误”的语义问题，如把纯文本模型标成支持图像、
  上下文窗口填错）。
- git 是审计轨迹。
- release 晋升（prerelease 翻 stable）是最终发布门控，和 `sync-r2.yml` 同一道门。
- **签名只保证完整性，不代替审核**：签名挡得住篡改 / 伪造发布方，挡不住发布方自己填错。
  所以审核对“安全”（不只是质量）是必要的。

### G3：谁维护 = 谁的密钥签名

- 维护方 / 发布方 = BigStrongSun（你）。信任锚 = minisign 公钥（已在 Tauri updater 配置里）。
- 客户端验签，所以“谁维护”编码为“谁的 key 签”：没有 key 就注入不了条目。
- 要共享维护时：TUF 式多密钥阈值（如 2-of-3），避免单人可单方面发布。属 P3。

### 待确认决策

- 目录源位置：主仓库 vs 独立仓库（建议：先主仓库，要社区再拆）。
- 签名格式：minisign（复用现有 key + 客户端验签 + 发布管线）vs 本次实现的裸 Ed25519。
  建议：生产用 minisign，与现有 updater 一致；本次 Ed25519 地基仍有效，切换只是签名格式
  drop-in（传输 + 校验逻辑不变）。
- 单维护方 vs 社区（建议：先单维护方，审核 = 你的 PR 评审 + release 门控）。

## 本次交付（可验证地基）

1. `services/preset_registry.rs`：
   - `PresetSourceKind`（`webdav` / `https`）、`PresetSource`、`PresetRegistrySettings`。
   - `PresetManifest`（schemaVersion、version、publishedAt、expiresAt、target、sha256、size、
     signature、changelog）。
   - `validate_manifest()`：纯函数，校验 size / SHA-256 / 过期 / 版本不回退 / Ed25519 签名。
   - `fetch_preset_manifest()`：WebDAV 传输，复用 `webdav.rs` 原语，下载后走 `validate_manifest`。
2. `settings.rs`：`AppSettings.preset_registry: Option<PresetRegistrySettings>` + get/set。
3. `services/mod.rs`：注册模块。
4. 测试：合法 manifest 接受；坏 hash / 过期 / 版本回退 / 坏签名 均拒绝；WebDAV 传输路径。
5. `commands/preset_registry.rs` + `lib.rs`：Tauri 命令入口，使地基可达（非死代码）：
   - `preset_registry_get_settings` / `preset_registry_save_settings`：设备级源配置管理。
   - `preset_registry_check_update(sourceId)`：拉取并校验 manifest，返回版本/变更摘要/是否有更新，不应用。
   - `https` 源本次返回明确“未实现”错误（联调仅交付 WebDAV 传输）。

## 验收

- `cargo test preset_registry` 全绿。
- 篡改 hash / 签名 / 过期 / 降级版本均被拒绝（测试覆盖）。
- 离线时不改变任何 DB / 缓存（本次仅校验，不落地应用，故天然满足）。
- 不引入裸 URL 无签名下载路径（`pinned-key` 源必须带签名）。

## 后续（不在本次）

- P1：本地可移植预设（版本化 schema、脱敏导出、文件导入、diff、三方合并、事务、备份、回滚）。
- P2 完整：缓存与过期策略、检查更新 UI、下载失败回退、TUF 多角色（root/targets/snapshot/timestamp）。
- 前端：预设源管理、版本状态、diff、冲突逐项选择。
