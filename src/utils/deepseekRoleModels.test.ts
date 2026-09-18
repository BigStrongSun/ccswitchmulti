import { describe, expect, it } from "vitest";

import {
  catalogHasRoleModel,
  deepSeekRoleForModel,
  deepSeekRoleModelsMatch,
} from "./deepseekRoleModels";

describe("deepSeekRoleForModel", () => {
  it("maps official alias slugs to the flash and pro roles", () => {
    expect(deepSeekRoleForModel("deepseek-flash")).toBe("flash");
    expect(deepSeekRoleForModel("DeepSeek-Flash")).toBe("flash");
    expect(deepSeekRoleForModel("deepseek-pro")).toBe("pro");
    expect(deepSeekRoleForModel("deepseek-v4-flash")).toBe("flash");
    expect(deepSeekRoleForModel("deepseek-v4-pro")).toBe("pro");
    expect(deepSeekRoleForModel("deepseek-v4-flash-202605")).toBe("flash");
    expect(deepSeekRoleForModel("deepseek-flash-0731")).toBe("flash");
  });

  it("excludes vision models and unrelated slugs", () => {
    expect(deepSeekRoleForModel("deepseek-v4-flash-vision-exp")).toBeNull();
    expect(deepSeekRoleForModel("deepseek-flash-vision")).toBeNull();
    expect(deepSeekRoleForModel("deepseek-chat")).toBeNull();
    expect(deepSeekRoleForModel("gpt-5.6-sol")).toBeNull();
    expect(deepSeekRoleForModel("")).toBeNull();
  });
});

describe("deepSeekRoleModelsMatch", () => {
  it("treats alias and canonical slugs as the same model", () => {
    expect(deepSeekRoleModelsMatch("deepseek-flash", "deepseek-v4-flash")).toBe(
      true,
    );
    expect(deepSeekRoleModelsMatch("deepseek-v4-flash", "deepseek-flash")).toBe(
      true,
    );
    expect(deepSeekRoleModelsMatch("deepseek-pro", "deepseek-v4-pro")).toBe(
      true,
    );
    expect(deepSeekRoleModelsMatch("QWEN3.8", "qwen3.8")).toBe(true);
  });

  it("never matches across roles or unrelated models", () => {
    expect(deepSeekRoleModelsMatch("deepseek-flash", "deepseek-v4-pro")).toBe(
      false,
    );
    expect(
      deepSeekRoleModelsMatch("deepseek-v4-flash-vision-exp", "deepseek-flash"),
    ).toBe(false);
    expect(deepSeekRoleModelsMatch("qwen3.8", "qwen3.6")).toBe(false);
  });
});

describe("catalogHasRoleModel", () => {
  it("finds role models through the official alias", () => {
    expect(
      catalogHasRoleModel([{ model: "deepseek-flash" }], "deepseek-v4-flash"),
    ).toBe(true);
    expect(
      catalogHasRoleModel(
        [{ model: "deepseek-v4-flash" }],
        "deepseek-v4-flash",
      ),
    ).toBe(true);
    expect(catalogHasRoleModel([], "deepseek-v4-flash")).toBe(false);
  });
});
