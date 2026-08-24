# PLAN: Codex Reasoning 档位修复 — 窄显示 + 宽映射兜底 + 官方档位继承

> 目标：修复 CCSwitchMulti（Tauri v3.19.1-28）两个 reasoning 档位问题。
> 执行模型：deepseek-v4-flash。本 plan 自包含，不需要额外上下文，照做即可。
> 原则：**窄显示**（投影/菜单只显示模型真实支持档位）+ **宽映射兜底**（代理端仍接受映射档位，不破坏现有请求）。

---

## 0. 背景（为什么这么改）

当前 `resolve_subagent_reasoning_capability` 的 `codex_selectable_efforts`（可选档位集合）被算成了 6 档：

```
deepseek-v4-flash/pro / k3 / k3-256k 的 builtin 能力：
  supported_efforts = [low, high, max]          ← 真实能力只有 3 档
  effort_map        = {low→low, medium→high, high→high, xhigh→high, max→max}  ← 上游映射表
selectable 计算 = effort_map.keys() ∪ {none}   ← 把映射源档位 medium/xhigh 也当成了可选档位
结果 = [none, low, medium, high, xhigh, max]    ← 6 档，但真实只有 3 档
```

两个后果：
1. **投影文件**（`~/.codex/cc-switch-model-catalog.json`）里 deepseek/k3 显示 6 档，但 DeepSeek V4 官方 API 只接受 low/high/max，medium/xhigh 是「虚假档位」。
2. **luna/sol 等官方模型**（gpt-5.6-luna、gpt-5.6-sol）模型行 `reasoning=null`、builtin 不认 → 能力解析为 Unknown → 子智能体编辑器配不了档位，尽管官方缓存里有它们的档位。

但 `codex_selectable_efforts` 不止用于渲染菜单，它还驱动**代理层的档位校验**（见下文），所以不能简单删掉 medium/xhigh，否则 Codex 端一旦发 medium/xhigh 就会报错。本 plan 用「窄显示 + 宽映射兜底」解决这个矛盾。

---

## 1. 改动清单总览

| # | 文件 | 函数 | 改动 |
|---|------|------|------|
| A1 | `src-tauri/src/proxy/providers/codex_reasoning.rs` | `resolve_subagent_reasoning_capability` | selectable 改真实能力（窄） |
| A2 | `src-tauri/src/proxy/providers/codex.rs` | `encode_codex_capability_effort_mode` | mappings 改遍历完整 effort_map（宽） |
| A3 | `src-tauri/src/proxy/providers/transform_codex_chat.rs` | `map_capability_reasoning_effort` | 先映射后校验（兜底） |
| B1 | `src-tauri/src/codex_config.rs` | 新增 `codex_official_models_cache` + `official_reasoning_capability_for_model` | 官方档位来源 |
| B2 | `src-tauri/src/codex_config.rs` | `codex_catalog_model_specs` | 解析链加第 4 来源 |

---

## 2. A1 — selectable 收窄（真实能力档位）

文件：`src-tauri/src/proxy/providers/codex_reasoning.rs`

`resolve_subagent_reasoning_capability`（约 209-221 行），把：

```rust
    let selectable_set = effort_map
        .keys()
        .copied()
        .chain(
            capability
                .disable_allowed
                .then_some(CodexReasoningEffort::None),
        )
        .collect::<HashSet<_>>();
    let codex_selectable_efforts = CodexReasoningEffort::ORDERED
        .into_iter()
        .filter(|effort| selectable_set.contains(effort))
        .collect();
```

改成（用已经算好的 `provider_effort_set`，它在 188-191 行已定义，是真实能力的集合）：

```rust
    let selectable_set = provider_effort_set
        .clone()
        .into_iter()
        .chain(
            capability
                .disable_allowed
                .then_some(CodexReasoningEffort::None),
        )
        .collect::<HashSet<_>>();
    let codex_selectable_efforts = CodexReasoningEffort::ORDERED
        .into_iter()
        .filter(|effort| selectable_set.contains(effort))
        .collect();
```

要点：
- **不要动** `effort_map` 的构建逻辑（179-207 行），它仍是完整映射（含 medium→high、xhigh→high）。`effort_map` 供代理层做宽映射兜底用。
- `provider_effort_set` 是 `HashSet<CodexReasoningEffort>`，由 `provider_accepted_efforts`（真实能力，按 `ORDERED` 过滤 `supported_efforts` 得到）构建。deepseek/k3 会得到 `{low, high, max}`。
- `disable_allowed=true` 时保留 `None`（作为「关闭推理」开关，不是档位）。deepseek/k3 builtin 的 `disable_allowed=true`，所以最终 selectable = `[none, low, high, max]`。

---

## 3. A2 — effort_value_mode 的 mappings 改遍历完整 effort_map

文件：`src-tauri/src/proxy/providers/codex.rs`

`encode_codex_capability_effort_mode`（约 1919-1940 行）。当前 mappings 只遍历 selectable（`supported_efforts` 参数），改成遍历完整的 `effort_map`：

当前：
```rust
fn encode_codex_capability_effort_mode(
    supported_efforts: &[super::codex_reasoning::CodexReasoningEffort],
    effort_map: &std::collections::BTreeMap<
        super::codex_reasoning::CodexReasoningEffort,
        super::codex_reasoning::CodexReasoningEffort,
    >,
) -> String {
    let allowed = supported_efforts
        .iter()
        .map(|effort| effort.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mappings = supported_efforts
        .iter()
        .map(|effort| {
            let mapped = effort_map.get(effort).unwrap_or(effort);
            format!("{}={}", effort.as_str(), mapped.as_str())
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("capability|{allowed}|{mappings}")
}
```

改成（`allowed` 保持窄 = selectable，`mappings` 变宽 = 完整 effort_map）：

```rust
fn encode_codex_capability_effort_mode(
    supported_efforts: &[super::codex_reasoning::CodexReasoningEffort],
    effort_map: &std::collections::BTreeMap<
        super::codex_reasoning::CodexReasoningEffort,
        super::codex_reasoning::CodexReasoningEffort,
    >,
) -> String {
    let allowed = supported_efforts
        .iter()
        .map(|effort| effort.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mappings = effort_map
        .iter()
        .map(|(source, target)| format!("{}={}", source.as_str(), target.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    format!("capability|{allowed}|{mappings}")
}
```

结果示例（deepseek）：`capability|none,low,high,max|low=low,medium=high,high=high,xhigh=high,max=max`
- `allowed` = 窄（真实档位 + none）
- `mappings` = 宽（含 medium→high、xhigh→high 兜底映射）

---

## 4. A3 — 代理端「先映射后校验」

文件：`src-tauri/src/proxy/providers/transform_codex_chat.rs`

`map_capability_reasoning_effort`（约 884-902 行）。当前先校验 allowed（窄），medium/xhigh 会 fail closed。改成**先查映射（宽兜底），查不到再校验 allowed（窄）**：

当前：
```rust
fn map_capability_reasoning_effort<'a>(
    effort: &'a str,
    mode: &'a str,
) -> Result<&'a str, ProxyError> {
    let mut sections = mode.splitn(3, '|');
    let _kind = sections.next();
    let allowed = sections.next().unwrap_or_default();
    let mappings = sections.next().unwrap_or_default();
    if !allowed.split(',').any(|candidate| candidate == effort) {
        return Err(ProxyError::TransformError(format!(
            "reasoning effort `{effort}` is not supported; allowed=[{allowed}]"
        )));
    }
    Ok(mappings
        .split(',')
        .filter_map(|mapping| mapping.split_once('='))
        .find_map(|(source, target)| (source == effort).then_some(target))
        .unwrap_or(effort))
}
```

改成：
```rust
fn map_capability_reasoning_effort<'a>(
    effort: &'a str,
    mode: &'a str,
) -> Result<&'a str, ProxyError> {
    let mut sections = mode.splitn(3, '|');
    let _kind = sections.next();
    let allowed = sections.next().unwrap_or_default();
    let mappings = sections.next().unwrap_or_default();
    // 宽映射兜底：medium/xhigh 等映射档位直接命中，返回上游档位（如 high）
    if let Some(target) = mappings
        .split(',')
        .filter_map(|mapping| mapping.split_once('='))
        .find_map(|(source, target)| (source == effort).then_some(target))
    {
        return Ok(target);
    }
    // 窄校验：无映射时，effort 必须在 allowed 内，否则报错
    if !allowed.split(',').any(|candidate| candidate == effort) {
        return Err(ProxyError::TransformError(format!(
            "reasoning effort `{effort}` is not supported; allowed=[{allowed}]"
        )));
    }
    Ok(effort)
}
```

语义验证：
- `medium` → mappings 命中 `medium=high` → 返回 `high`（兜底映射，不报错）
- `xhigh` → 命中 `xhigh=high` → 返回 `high`
- `low` / `high` / `max` → 命中 identity 映射（或 allowed）→ 原值
- `none` → 无映射，allowed 含 none → 返回 none（实际上层 `reasoning_requested` 已提前处理 none，正常到不了这里）
- 未知档位（如 `foo`）→ 无映射、不在 allowed → 报错（保持 fail closed 语义）

---

## 5. B1 — 官方档位来源

文件：`src-tauri/src/codex_config.rs`

### 5.1 新增 helper：读官方缓存 models 数组

复用现有 `enrich_codex_catalog_with_official_metadata`（约 4362-4390 行）里「cc_switch_owned 时用 backup 文件」的逻辑，抽一个只读 helper。放在 `get_codex_models_cache_backup_path`（约 4153 行）附近：

```rust
/// 读取 Codex 官方模型缓存（models 数组）。
///
/// CC Switch 接管后会把路由目录写进 models_cache.json（etag 标记为 CC_SWITCH 拥有），
/// 官方原始档位在 backup 文件里。此处与 enrich_codex_catalog_with_official_metadata
/// 保持同一选择逻辑：缓存被 CC Switch 拥有时优先读 backup。
/// 任何读取/解析失败都返回 None（静默降级，不阻断投影）。
fn codex_official_models_cache() -> Option<Vec<Value>> {
    let cache_path = get_codex_models_cache_path();
    let backup_path = get_codex_models_cache_backup_path();
    let existing_cache = read_json_file_if_exists(&cache_path).ok().flatten()?;
    let official_cache = match existing_cache.as_ref() {
        Some(cache) if codex_models_cache_is_cc_switch_owned(cache) => {
            read_json_file_if_exists(&backup_path)
                .ok()
                .flatten()
                .or_else(|| existing_cache.clone())
        }
        _ => existing_cache,
    }?;
    let models = official_cache
        .get("models")
        .and_then(Value::as_array)?
        .clone();
    Some(models)
}
```

> 注意：`read_json_file_if_exists` 返回 `Result<Option<Value>, AppError>`（见 4163 行）。上面的 `.ok().flatten()` 把 `Result<Option<Value>>` 转成 `Option<Value>`，任何错误都降级为 None。

### 5.2 新增函数：从官方缓存构造 capability

放在同文件、`codex_catalog_model_specs` 之前（约 1360 行前）：

```rust
/// 从 Codex 官方缓存为指定 slug 构造 reasoning capability。
///
/// 官方缓存字段是 snake_case，`supported_reasoning_levels` 是字符串数组
/// （["low","medium",...]），不是投影文件里 {effort,description} 的对象数组。
fn official_reasoning_capability_for_model(
    model: &str,
    official_models: &[Value],
) -> Option<crate::proxy::providers::codex_reasoning::CodexModelReasoningCapability> {
    use crate::proxy::providers::codex_reasoning::{
        CodexModelReasoningCapability, CodexModelReasoningUpstream,
    };
    let entry = official_models.iter().find(|entry| {
        entry
            .get("slug")
            .and_then(Value::as_str)
            .is_some_and(|slug| slug.eq_ignore_ascii_case(model))
    })?;
    let levels: Vec<String> = entry
        .get("supported_reasoning_levels")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|level| level.as_str().map(str::trim).map(ToString::to_string))
        .filter(|level| !level.is_empty())
        .collect();
    if levels.is_empty() {
        return None;
    }
    let default_effort = entry
        .get("default_reasoning_level")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|level| !level.is_empty())
        .map(ToString::to_string);
    let capability = CodexModelReasoningCapability {
        supported: true,
        supported_efforts: levels.clone(),
        default_effort,
        disable_allowed: false,
        upstream: CodexModelReasoningUpstream {
            format: "string".to_string(),
            parameter: "reasoning_effort".to_string(),
            effort_map: levels
                .into_iter()
                .map(|level| (level.clone(), level))
                .collect(),
        },
        output_format: None,
        source: Some("official".to_string()),
    };
    capability.validate().ok()?;
    Some(capability)
}
```

要点：
- **字段名是 snake_case**（`supported_reasoning_levels` / `default_reasoning_level`），且 levels 是**字符串数组**。
- 用 `slug` 精确匹配（官方缓存里 luna 的 slug = `gpt-5.6-luna`，与 subagentV2 的 model 名一致）。
- `effort_map` 用 identity（官方 GPT 模型走 OpenAI 顶层 `reasoning_effort` 字段，无需映射）。
- `validate().ok()?` 保守降级：若官方数据 `default` 不在 `levels` 里（异常数据），返回 None → 该模型仍 Unknown，不产生虚假档位。
- 官方 levels 不含 `none`，所以 `disable_allowed=false`（信任缓存，不擅自加关闭选项）。

---

## 6. B2 — 接入解析链

文件：`src-tauri/src/codex_config.rs`

`codex_catalog_model_specs`（约 1361 行起）。在函数开头（`let mut specs = Vec::new();` 之后、`for model_config in models` 循环之前）读一次官方缓存：

```rust
    let mut seen = std::collections::HashSet::new();
    let mut specs = Vec::new();
    let official_models = codex_official_models_cache().unwrap_or_default();
```

然后把 reasoning 解析链（约 1455-1468 行）加第 4 来源：

当前：
```rust
        let reasoning =
            crate::proxy::providers::codex_reasoning::reasoning_capability_from_model_entry(
                model_config,
            )
            .or_else(|| {
                crate::proxy::providers::codex_reasoning::builtin_reasoning_capability_for_model(
                    model,
                )
            })
            .or_else(|| {
                upstream_model.as_deref().and_then(
                    crate::proxy::providers::codex_reasoning::builtin_reasoning_capability_for_model,
                )
            });
```

改成（链尾加 `official_reasoning_capability_for_model`）：
```rust
        let reasoning =
            crate::proxy::providers::codex_reasoning::reasoning_capability_from_model_entry(
                model_config,
            )
            .or_else(|| {
                crate::proxy::providers::codex_reasoning::builtin_reasoning_capability_for_model(
                    model,
                )
            })
            .or_else(|| {
                upstream_model.as_deref().and_then(
                    crate::proxy::providers::codex_reasoning::builtin_reasoning_capability_for_model,
                )
            })
            .or_else(|| official_reasoning_capability_for_model(model, &official_models));
```

优先级：①模型行声明 > ②builtin(model) > ③builtin(upstream_model) > ④官方缓存。
- deepseek/k3 在 ② 命中，不查 ④（官方缓存无 deepseek，查也 None）。
- luna/sol 在 ②③ 不命中 → ④ 命中官方档位。

---

## 7. 测试

### 7.1 更新现有测试（A1 连带）

文件：`src-tauri/src/proxy/providers/codex_reasoning.rs` 测试模块（约 385-403 行）。

`deepseek_resolution_separates_provider_and_codex_efforts` 的 selectable 断言从 6 档改 4 档：

```rust
        assert_eq!(
            resolved.codex_selectable_efforts,
            efforts(&["none", "low", "high", "max"])
        );
```

`effort_map` 断言（397-399 行 `Medium→High`）**保持不变**——effort_map 仍是完整映射。

### 7.2 新增：宽映射兜底（A2+A3）

文件：`src-tauri/src/proxy/providers/codex.rs` 测试模块，新增：

```rust
    #[test]
    fn capability_effort_mode_keeps_wide_mappings_for_narrow_selectable() {
        use super::super::codex_reasoning::{
            builtin_reasoning_capability_for_model, resolve_subagent_reasoning_capability,
        };
        let capability = builtin_reasoning_capability_for_model("deepseek-v4-flash")
            .expect("deepseek builtin");
        let resolved = resolve_subagent_reasoning_capability(Some(&capability));
        let mode = encode_codex_capability_effort_mode(
            &resolved.codex_selectable_efforts,
            &resolved.effort_map,
        );
        // allowed 收窄，mappings 保留 medium/xhigh 映射
        assert_eq!(
            mode,
            "capability|none,low,high,max|low=low,medium=high,high=high,xhigh=high,max=max"
        );
    }
```

文件：`src-tauri/src/proxy/providers/transform_codex_chat.rs` 测试模块，新增（验证 medium/xhigh 兜底映射，unknown 仍报错）：

```rust
    #[test]
    fn capability_effort_mapping_narrow_display_wide_remap() {
        let mode = "capability|none,low,high,max|low=low,medium=high,high=high,xhigh=high,max=max";
        assert_eq!(map_capability_reasoning_effort("medium", mode).unwrap(), "high");
        assert_eq!(map_capability_reasoning_effort("xhigh", mode).unwrap(), "high");
        assert_eq!(map_capability_reasoning_effort("low", mode).unwrap(), "low");
        assert_eq!(map_capability_reasoning_effort("max", mode).unwrap(), "max");
        assert!(map_capability_reasoning_effort("foo", mode).is_err());
    }
```

> 注意：`map_capability_reasoning_effort` 是私有函数，测试需放在 `transform_codex_chat.rs` 的 `#[cfg(test)] mod tests` 内。

### 7.3 新增：官方档位继承（B1+B2）

文件：`src-tauri/src/codex_config.rs` 测试模块，新增：

```rust
    #[test]
    fn official_reasoning_capability_reads_snake_case_levels() {
        use crate::proxy::providers::codex_reasoning::CodexModelReasoningCapability;
        let official = serde_json::json!([{
            "slug": "gpt-5.6-luna",
            "supported_reasoning_levels": ["low", "medium", "high", "xhigh", "max"],
            "default_reasoning_level": "medium"
        }]);
        let models = official.as_array().unwrap().clone();
        let capability = official_reasoning_capability_for_model("gpt-5.6-luna", &models)
            .expect("luna official capability");
        assert_eq!(
            capability.supported_efforts,
            vec!["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(capability.default_effort.as_deref(), Some("medium"));
        assert_eq!(capability.source.as_deref(), Some("official"));
        // 不匹配的 slug 返回 None
        assert!(official_reasoning_capability_for_model("gpt-5.6-sol", &models).is_none());
    }
```

---

## 8. 验证

1. `cargo build --release` 通过（无编译错误/新 warning）。
2. `cargo test -p cc-switch codex_reasoning` 全绿（含更新后的 selectable 断言）。
3. `cargo test -p cc-switch capability_effort` 全绿（宽映射兜底）。
4. `cargo test -p cc-switch official_reasoning` 全绿（官方档位继承）。
5. `cargo test -p cc-switch`（全量，确认无回归；首次并行编译偶发级联报错可增量重跑）。

行为验证（改完重新 `tauri build --no-bundle` 后，退出旧 CCSM 跑新 exe）：
- 投影 `~/.codex/cc-switch-model-catalog.json` 里 deepseek/k3 变 `[none, low, high, max]`（不再是 6 档）。
- 子智能体编辑器里 luna/sol 出现官方档位（luna 5 档、sol 6 档含 ultra），可切 fixed 配档位。
- 代理端：即使 Codex 端仍发 medium/xhigh，映射到 high 转发，不报错。

---

## 9. 坑与注意事项（务必遵守）

1. **A1 不要动 `effort_map` 构建逻辑**（codex_reasoning.rs 179-207 行）。effort_map 必须保持完整（含 medium→high/xhigh→high），它是 A2/A3 宽映射兜底的数据源。
2. **B1 必须走 backup 文件**：CC Switch 接管后 models_cache.json 是 CCSM 写的（etag=CC_SWITCH 标记），官方原始档位在 `models_cache.cc-switch-backup.json`。读错文件会导致 luna/sol 拿不到官方档位。
3. **官方缓存字段是 snake_case 字符串数组**，不是投影文件的 `{effort,description}` 对象数组，别用错解析逻辑。
4. **官方 levels 含 `ultra`**（sol/terra），`VALID_EFFORTS` 和 `CodexReasoningEffort` 枚举已支持，但构造后必须过 `validate()`（`default` 必须在 `supported_efforts` 里）。
5. **source="official" 会落 confidence=Unverified**（resolve 里只有 builtin→Confirmed / user→Declared）。这是可接受现状；如产品要求官方档位显示「已确认」，另行在 `resolve_subagent_reasoning_capability` 的 confidence match 里给 `"official"` 加分支（本 plan 不要求）。
6. **A2/A3 是一对**，必须一起改，否则 `mappings` 里没有 medium/xhigh，A3 的兜底查不到映射会退回窄校验报错。
7. **现有测试会断**：`codex_reasoning.rs:393-394` 的 selectable 断言必须同步更新（见 7.1），否则 `cargo test` 失败。
