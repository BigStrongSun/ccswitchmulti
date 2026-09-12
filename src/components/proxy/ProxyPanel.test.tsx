import { describe, expect, it } from "vitest";

import { listenerRecoveryButtonState } from "./ProxyPanel";

describe("listenerRecoveryButtonState", () => {
  it("keeps the recovery control visible but only enables it for an active takeover", () => {
    expect(listenerRecoveryButtonState(undefined)).toEqual({
      enabled: false,
      detail: "先启用至少一个应用接管后才能恢复监听。",
    });

    expect(
      listenerRecoveryButtonState({
        claude: false,
        codex: true,
        gemini: false,
        grokbuild: false,
        opencode: false,
        openclaw: false,
        hermes: false,
      }),
    ).toEqual({
      enabled: true,
      detail: "持续守护当前配置的监听端口；仅自动释放已验证的旧 CCSM 实例。",
    });
  });
});
