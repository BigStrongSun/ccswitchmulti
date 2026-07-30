import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

const requiredKeys = [
  "codexOauth.poolAutoSwitch",
  "codexOauth.poolDescription",
  "codexOauth.poolRemaining",
  "codexOauth.poolReserveAria",
  "codexOauth.poolSave",
  "codexOauth.facadeNativeMixed",
  "codexOauth.facadeManaged",
  "codexOauth.facadeRestartNotice",
  "oauthDelete.allTitle",
  "oauthDelete.accountMessage",
  "oauthDelete.removeAll",
  "codexRouterAuth.label",
  "codexRouterAuth.desktopOption",
  "codexRouterAuth.defaultAccount",
  "codexRouterAuth.defaultAccountWithId",
  "codexRouterAuth.poolHint",
  "codexRouterAuth.facadePending",
  "codexRouterAuth.restartNotice",
  "providerPreset.expandAll",
  "providerPreset.collapse",
] as const;

type TranslationTree = Record<string, unknown>;

function readTranslation(tree: TranslationTree, path: string): unknown {
  return path.split(".").reduce<unknown>((value, segment) => {
    if (typeof value !== "object" || value === null) return undefined;
    return (value as TranslationTree)[segment];
  }, tree);
}

describe("Codex authentication UI locale coverage", () => {
  it.each([
    ["zh", zh],
    ["zh-TW", zhTW],
    ["en", en],
    ["ja", ja],
  ])("defines every required key in %s", (_locale, translations) => {
    const missing = requiredKeys.filter((key) => {
      const value = readTranslation(translations, key);
      return typeof value !== "string" || value.trim().length === 0;
    });

    expect(missing).toEqual([]);
  });

  it("uses locale-specific facade labels outside Simplified Chinese", () => {
    expect(en.codexOauth.facadeManaged).toBe("CCSM managed authentication");
    expect(ja.codexOauth.facadeManaged).toBe("CCSM 管理認証");
    expect(zhTW.codexOauth.facadeManaged).toBe("CCSM 代管認證");
  });
});
