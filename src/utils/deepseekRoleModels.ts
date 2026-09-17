export type DeepSeekRole = "flash" | "pro";

/// DeepSeek 角色模型 slug 族：官方 API 现行 slug（deepseek-flash / deepseek-pro）
/// 与产品内 canonical slug（deepseek-v4-flash / deepseek-v4-pro，含 -202605 / -0731
/// 等日期版本）指向同一模型；*-vision* 是独立视觉模型，不属于文本角色族。
/// 与 src-tauri 的 deepseek_role_identity_for_model 保持同一口径。
export function deepSeekRoleForModel(name: string): DeepSeekRole | null {
  const normalized = name.trim().toLowerCase();
  if (normalized.includes("vision")) return null;
  if (
    normalized === "deepseek-flash" ||
    normalized === "deepseek-v4-flash" ||
    normalized.startsWith("deepseek-flash-") ||
    normalized.startsWith("deepseek-v4-flash-")
  ) {
    return "flash";
  }
  if (
    normalized === "deepseek-pro" ||
    normalized === "deepseek-v4-pro" ||
    normalized.startsWith("deepseek-pro-") ||
    normalized.startsWith("deepseek-v4-pro-")
  ) {
    return "pro";
  }
  return null;
}

/// 两个模型 slug 是否指向同一可路由模型：精确匹配（忽略大小写）
/// 或同属一个 DeepSeek 角色族（别名等价）。
export function deepSeekRoleModelsMatch(a: string, b: string): boolean {
  if (a.trim().toLowerCase() === b.trim().toLowerCase()) return true;
  const aRole = deepSeekRoleForModel(a);
  const bRole = deepSeekRoleForModel(b);
  return aRole !== null && aRole === bRole;
}

/// catalog 里是否存在与目标 slug 同一可路由模型的条目（含 DeepSeek 别名）。
export function catalogHasRoleModel(
  models: Array<{ model?: string }>,
  target: string,
): boolean {
  return models.some((model) =>
    deepSeekRoleModelsMatch(model.model?.trim() ?? "", target),
  );
}
