/**
 * MultiRouter 聚合模型目录的排序 SSOT。
 *
 * 聚合目录会因为删除供应商、路由增删、`/models` 刷新而整体重建。如果顺序完全由
 * “route 迭代顺序 × 目标 provider 目录顺序”推导，任何集合变化都会让老模型跳位。
 * 这里统一按上一次目录的相对顺序补位，真正新增的模型追加到末尾。
 */

export interface CodexOrderableCatalogModel {
  model?: string;
  sortIndex?: number;
}

/**
 * 读取上一次目录的有效顺序：`sortIndex` 优先，缺失时退回数组下标。
 *
 * 返回“模型 -> 名次”映射；重复模型只记录第一次出现的名次。
 */
function buildPreviousRankByModel(
  previousModels: readonly CodexOrderableCatalogModel[],
): Map<string, number> {
  const ranked = previousModels
    .map((model, index) => ({
      id: model.model?.trim() ?? "",
      index,
      sortIndex: model.sortIndex,
    }))
    .filter((entry) => Boolean(entry.id))
    .sort(
      (left, right) =>
        (left.sortIndex ?? Number.MAX_SAFE_INTEGER) -
          (right.sortIndex ?? Number.MAX_SAFE_INTEGER) ||
        left.index - right.index,
    );

  const rankByModel = new Map<string, number>();
  for (const entry of ranked) {
    if (!rankByModel.has(entry.id)) rankByModel.set(entry.id, rankByModel.size);
  }
  return rankByModel;
}

/** 判断目录是否启用过自定义排序（存在任意 `sortIndex`）。 */
export function hasCustomCatalogOrder(
  models: readonly CodexOrderableCatalogModel[],
): boolean {
  return models.some((model) => model.sortIndex !== undefined);
}

/**
 * 按当前数组顺序落地 `sortIndex`。
 *
 * 启用自定义排序时写入稠密序号（0 起），删除中间模型不留空洞、新增模型拿到末尾序号；
 * 未启用时清掉可能从上游 provider 继承来的 `sortIndex`，让数组顺序继续表达默认顺序。
 */
function writeCatalogSortIndexes<T extends CodexOrderableCatalogModel>(
  models: readonly T[],
  useCustomOrder: boolean,
): T[] {
  if (useCustomOrder) {
    return models.map((model, index) => ({ ...model, sortIndex: index }));
  }
  return models.map((model) => {
    if (model.sortIndex === undefined) return model;
    const { sortIndex: _sortIndex, ...rest } = model;
    return rest as T;
  });
}

/**
 * 按上一次目录顺序重排重建结果，并把新增模型追加到末尾。
 *
 * 上一次目录使用过自定义排序时（存在任意 `sortIndex`），返回值会为全部模型写入
 * 稠密 `sortIndex`（0 起），这样删除中间模型不会留下空洞，新增模型也拿到末尾序号，
 * 而不是回落到后端的默认供应商启发式排序。上一次目录没有自定义排序时保持“无
 * `sortIndex`”语义，只由数组顺序表达默认顺序，让“恢复默认”继续生效。
 *
 * 上游 provider 目录里的 `sortIndex` 不参与聚合排序：聚合目录的顺序偏好只属于
 * MultiRouter 自身，否则模型源刷新会把无关序号带进方案。
 */
export function applyCodexCatalogModelOrder<
  T extends CodexOrderableCatalogModel,
>(
  nextModels: readonly T[],
  previousModels: readonly CodexOrderableCatalogModel[],
): T[] {
  const rankByModel = buildPreviousRankByModel(previousModels);
  const ordered = nextModels
    .map((model, index) => ({
      model,
      index,
      rank: rankByModel.get(model.model?.trim() ?? ""),
    }))
    .sort(
      (left, right) =>
        (left.rank ?? Number.MAX_SAFE_INTEGER) -
          (right.rank ?? Number.MAX_SAFE_INTEGER) || left.index - right.index,
    )
    .map((entry) => entry.model);

  return writeCatalogSortIndexes(
    ordered,
    hasCustomCatalogOrder(previousModels),
  );
}

/**
 * 用户在向导里显式给出顺序时，数组顺序即最终顺序，只需要落地 `sortIndex`。
 *
 * 上一次目录用过自定义排序就继续写稠密序号，否则清掉继承来的序号，避免后端
 * 因残留 `sortIndex` 与数组顺序冲突而回落到默认供应商启发式排序。
 */
export function applyCodexCatalogExplicitOrder<
  T extends CodexOrderableCatalogModel,
>(
  nextModels: readonly T[],
  previousModels: readonly CodexOrderableCatalogModel[],
): T[] {
  return writeCatalogSortIndexes(
    nextModels,
    hasCustomCatalogOrder(previousModels),
  );
}
