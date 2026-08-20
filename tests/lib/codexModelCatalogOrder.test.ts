import { describe, expect, it } from "vitest";
import { applyCodexCatalogModelOrder } from "@/lib/codexModelCatalogOrder";

describe("applyCodexCatalogModelOrder", () => {
  it("保留模型按上一次自定义顺序补位，并压缩 sortIndex 空洞", () => {
    const previous = [
      { model: "e", sortIndex: 0 },
      { model: "c", sortIndex: 1 },
      { model: "a", sortIndex: 2 },
      { model: "d", sortIndex: 3 },
      { model: "b", sortIndex: 4 },
    ];
    // 删除某个供应商后重建结果只剩它以外的模型，且顺序来自 route 迭代。
    const rebuilt = [{ model: "a" }, { model: "b" }, { model: "e" }];

    expect(applyCodexCatalogModelOrder(rebuilt, previous)).toEqual([
      { model: "e", sortIndex: 0 },
      { model: "a", sortIndex: 1 },
      { model: "b", sortIndex: 2 },
    ]);
  });

  it("新增模型统一追加到末尾", () => {
    const previous = [
      { model: "b", sortIndex: 0 },
      { model: "a", sortIndex: 1 },
    ];
    const rebuilt = [
      { model: "a" },
      { model: "new-1" },
      { model: "b" },
      { model: "new-2" },
    ];

    expect(applyCodexCatalogModelOrder(rebuilt, previous)).toEqual([
      { model: "b", sortIndex: 0 },
      { model: "a", sortIndex: 1 },
      { model: "new-1", sortIndex: 2 },
      { model: "new-2", sortIndex: 3 },
    ]);
  });

  it("上一次没有自定义排序时只按数组顺序补位，不写入 sortIndex", () => {
    const previous = [{ model: "e" }, { model: "c" }, { model: "a" }];
    const rebuilt = [{ model: "a" }, { model: "new" }, { model: "e" }];

    expect(applyCodexCatalogModelOrder(rebuilt, previous)).toEqual([
      { model: "e" },
      { model: "a" },
      { model: "new" },
    ]);
  });

  it("上游 provider 带来的 sortIndex 不会污染未自定义排序的聚合目录", () => {
    const previous = [{ model: "a" }, { model: "b" }];
    const rebuilt = [
      { model: "b", sortIndex: 7 },
      { model: "a", sortIndex: 3 },
    ];

    expect(applyCodexCatalogModelOrder(rebuilt, previous)).toEqual([
      { model: "a" },
      { model: "b" },
    ]);
  });
});
