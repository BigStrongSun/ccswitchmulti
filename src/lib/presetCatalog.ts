/**
 * 本地预设表（preset-table.json）前端访问层。
 *
 * bundle 由 `preset-table/tools/build_bundle.py` 生成，随 WebDAV 同步分发到
 * `~/.cc-switch/preset-table.json`。这里一次性加载后做同步查询，供
 * `codexModelContext.ts` 的上下文推断使用；加载失败时返回 null，调用方
 * 回退到内置硬编码预设。
 */
import { invoke } from "@tauri-apps/api/core";

export interface PresetTableBundle {
  schemaVersion: number;
  version: string;
  generatedAt: string;
  providers: Record<string, { api: string }>;
  baseline: Record<string, Record<string, unknown>>;
  plans: Record<string, Record<string, Record<string, unknown>>>;
  /** v2 source locks and deterministic hashes; absent on compatible v1 bundles. */
  manifest?: {
    sourceLocks?: Array<Record<string, unknown>>;
    layerHashes?: Record<string, string>;
  };
  /** Shared deployments only. Device-local endpoints and observations never appear here. */
  deployments?: Record<string, Record<string, unknown>>;
}

let cache: PresetTableBundle | null | undefined = undefined;

/**
 * 加载预设表（带模块级缓存）。
 *
 * `force=true` 时重新读取（WebDAV 下载完成后调用，拿到最新 bundle）。
 */
export async function loadPresetCatalog(
  force = false,
): Promise<PresetTableBundle | null> {
  if (cache !== undefined && !force) return cache;
  try {
    cache = await invoke<PresetTableBundle | null>("preset_catalog_get");
  } catch {
    cache = null;
  }
  return cache;
}

function entryContext(
  entry: Record<string, unknown> | undefined,
): number | undefined {
  const limit = entry?.limit as Record<string, unknown> | undefined;
  const context = limit?.context;
  return typeof context === "number" && context > 0 ? context : undefined;
}

function entryPercent(
  entry: Record<string, unknown> | undefined,
): number | undefined {
  const limit = entry?.limit as Record<string, unknown> | undefined;
  const percent = limit?.effective_context_percent;
  return typeof percent === "number" && percent >= 1 && percent <= 100
    ? percent
    : undefined;
}

function effectiveContext(
  entry: Record<string, unknown> | undefined,
): number | undefined {
  const context = entryContext(entry);
  if (context === undefined) return undefined;
  const percent = entryPercent(entry);
  return percent === undefined
    ? context
    : Math.floor((context * percent) / 100);
}

function deepMerge(
  base: Record<string, unknown>,
  override: Record<string, unknown>,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(override)) {
    const existing = out[key];
    if (
      existing &&
      typeof existing === "object" &&
      !Array.isArray(existing) &&
      value &&
      typeof value === "object" &&
      !Array.isArray(value)
    ) {
      out[key] = deepMerge(
        existing as Record<string, unknown>,
        value as Record<string, unknown>,
      );
    } else {
      out[key] = value;
    }
  }
  return out;
}

/**
 * Resolve an entire catalog entry for a known provider/model. This deliberately
 * does not invent request mappings: consumers may read transport or tool fields
 * only when a reviewed policy/deployment entry explicitly provides them.
 */
export function resolvePresetCatalogEntry(
  provider: string,
  modelId: string,
  plan?: string,
  deployment?: string,
): Record<string, unknown> | undefined {
  const bundle = cache;
  if (!bundle) return undefined;
  const model = modelId.trim();
  const providerId = provider.trim();
  if (!model || !providerId) return undefined;

  const baseKey = `${providerId}/${model}`;
  let entry = bundle.baseline[baseKey];
  if (!entry) return undefined;

  if (plan) {
    const policy = bundle.plans[plan]?.[model];
    if (!policy) return undefined;
    const policyBase =
      typeof policy.base_model === "string" ? policy.base_model : baseKey;
    if (policyBase !== baseKey) return undefined;
    entry = deepMerge(entry, policy);
  }

  if (deployment) {
    const overlay = bundle.deployments?.[deployment];
    if (!overlay || overlay.base_model !== baseKey) return undefined;
    entry = deepMerge(entry, overlay);
  }

  return entry;
}

/**
 * 同步查询预设表（仅使用已加载的缓存；未加载时返回 undefined）。
 *
 * - 传 `plan`：只查该 plan 的覆盖条目（基线 + 覆盖深合并），未命中不回落基线
 *   （plan 意图明确，避免把 API 数字套到订阅通道）。
 * - 不传 `plan`：按模型 ID 查基线（API 通道事实）。
 */
export function resolvePresetCatalogContextWindow(
  modelId: string,
  plan?: string,
): number | undefined {
  const bundle = cache;
  if (!bundle) return undefined;
  const model = modelId.trim();
  if (!model) return undefined;

  if (plan) {
    const planEntry = bundle.plans[plan]?.[model];
    if (!planEntry) return undefined;
    const baseKey =
      typeof planEntry.base_model === "string"
        ? planEntry.base_model
        : undefined;
    const base = baseKey ? bundle.baseline[baseKey] : undefined;
    return base ? effectiveContext(deepMerge(base, planEntry)) : undefined;
  }

  for (const [key, entry] of Object.entries(bundle.baseline)) {
    if (key.endsWith(`/${model}`)) {
      const context = effectiveContext(entry);
      if (context !== undefined) return context;
    }
  }
  return undefined;
}
