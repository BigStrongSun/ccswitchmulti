import { describe, expect, it } from "vitest";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zh from "./locales/zh.json";
import zhTW from "./locales/zh-TW.json";

const locales = { en, ja, zh, "zh-TW": zhTW } as const;
const visibleKinds = [
  "activePreviousInstance",
  "confirmedCrash",
  "uncleanExit",
  "livePreservedProviderRepaired",
  "providerOnlyRestored",
  "unrecoverableUserTables",
  "portOwnedByUnknownOwner",
  "startupTakeoverFailed",
] as const;
const nextSteps = [
  "closeOtherInstanceOrInspectProcess",
  "reviewRecoveryResults",
  "openLogsOrRestoreUserBackup",
  "changeProxyPortOrInspectOwner",
  "openLogsOrRetryTakeover",
] as const;

describe("recovery outcome translations", () => {
  it.each(Object.entries(locales))(
    "%s covers every user-visible outcome and action",
    (_locale, messages) => {
      for (const kind of visibleKinds) {
        expect(messages.notifications.recovery.kind[kind].trim()).not.toBe("");
      }
      for (const nextStep of nextSteps) {
        expect(
          messages.notifications.recovery.nextStep[nextStep].trim(),
        ).not.toBe("");
      }
    },
  );
});
