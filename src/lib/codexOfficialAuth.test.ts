import { describe, expect, it } from "vitest";
import type { Provider, ProviderMeta } from "@/types";
import {
  readCodexOfficialAuth,
  writeCodexOfficialAuth,
} from "./codexOfficialAuth";

function officialProvider(meta?: ProviderMeta): Provider {
  return {
    id: "codex-official",
    name: "OpenAI Official",
    category: "official",
    settingsConfig: { auth: {}, config: "" },
    ...(meta ? { meta } : {}),
  };
}

describe("OpenAI Official provider authentication ownership", () => {
  it("defaults missing provider authentication to the Desktop login without mutating input", () => {
    const provider = officialProvider();

    expect(readCodexOfficialAuth(provider)).toEqual({
      mode: "desktop_current_login",
    });
    expect(provider.meta).toBeUndefined();
  });

  it("normalizes a fixed managed account reference", () => {
    const provider = officialProvider({
      codexOfficialAuth: {
        mode: "managed_oauth",
        accountId: " account-1 ",
      },
    });

    expect(readCodexOfficialAuth(provider)).toEqual({
      mode: "managed_oauth",
      accountId: "account-1",
    });
  });

  it("drops a stale fixed account when switching to the account pool", () => {
    const provider = officialProvider({
      codexOfficialAuth: {
        mode: "managed_oauth",
        accountId: "account-1",
      },
      custom_endpoints: {
        primary: { url: "https://example.test", addedAt: 1 },
      },
    });

    const updated = writeCodexOfficialAuth(provider, { mode: "account_pool" });

    expect(updated.meta?.codexOfficialAuth).toEqual({ mode: "account_pool" });
    expect(updated.meta?.custom_endpoints).toEqual({
      primary: { url: "https://example.test", addedAt: 1 },
    });
    expect(provider.meta?.codexOfficialAuth).toEqual({
      mode: "managed_oauth",
      accountId: "account-1",
    });
  });
});
