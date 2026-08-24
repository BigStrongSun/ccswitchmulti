import { describe, expect, it } from "vitest";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zh from "./locales/zh.json";
import zhTW from "./locales/zh-TW.json";

const locales = { en, ja, zh, "zh-TW": zhTW } as const;

describe("startup recovery translations", () => {
  it.each(Object.entries(locales))(
    "%s provides a human-readable active-instance next step",
    (_locale, messages) => {
      const nextStep =
        messages.notifications.recoveryNextStep
          .closeOtherInstanceOrInspectProcess;

      expect(nextStep).toEqual(expect.any(String));
      expect(nextStep.trim()).not.toBe("");
      expect(nextStep).not.toBe("closeOtherInstanceOrInspectProcess");
    },
  );
});
