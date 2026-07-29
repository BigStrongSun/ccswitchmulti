import { describe, expect, it } from "vitest";
import {
  codexAccountPoolFacadeLabel,
  codexPoolFacadeRestartMessage,
} from "./CodexOAuthSection";

describe("Codex OAuth 账号池认证门面", () => {
  it("仅在账号池启用且 Desktop 项启用时展示混合认证", () => {
    const entries = [
      {
        accountId: "native_codex_auth",
        enabled: true,
        reservePercent: 5,
      },
      { accountId: "managed", enabled: true, reservePercent: 5 },
    ];
    expect(codexAccountPoolFacadeLabel({ enabled: true, entries })).toBe(
      "Desktop / 混合认证",
    );
    expect(codexAccountPoolFacadeLabel({ enabled: false, entries })).toBe(
      "CCSM 托管认证",
    );
    expect(
      codexAccountPoolFacadeLabel({
        enabled: true,
        entries: entries.map((entry) =>
          entry.accountId === "native_codex_auth"
            ? { ...entry, enabled: false }
            : entry,
        ),
      }),
    ).toBe("CCSM 托管认证");
  });

  it("只有门面发生变化时提示完全重启 Codex", () => {
    expect(
      codexPoolFacadeRestartMessage({
        applied: true,
        facadeChanged: true,
        codexRestartRequired: true,
        facade: "native_mixed",
      }),
    ).toContain("完全退出并重启 Codex");
    expect(
      codexPoolFacadeRestartMessage({
        applied: true,
        facadeChanged: false,
        codexRestartRequired: false,
        facade: "fully_managed",
      }),
    ).toBeNull();
  });
});
