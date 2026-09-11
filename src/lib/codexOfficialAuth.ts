import type {
  CodexOfficialAuthConfig,
  CodexOfficialAuthMode,
  Provider,
} from "@/types";

const DEFAULT_CODEX_OFFICIAL_AUTH: CodexOfficialAuthConfig = {
  mode: "desktop_current_login",
};

function isCodexOfficialAuthMode(
  value: unknown,
): value is CodexOfficialAuthMode {
  return (
    value === "desktop_current_login" ||
    value === "managed_oauth" ||
    value === "account_pool"
  );
}

export function normalizeCodexOfficialAuth(
  value: CodexOfficialAuthConfig | null | undefined,
): CodexOfficialAuthConfig {
  if (!value || !isCodexOfficialAuthMode(value.mode)) {
    return { ...DEFAULT_CODEX_OFFICIAL_AUTH };
  }
  if (value.mode !== "managed_oauth") {
    return { mode: value.mode };
  }
  const accountId = value.accountId?.trim();
  return {
    mode: "managed_oauth",
    ...(accountId ? { accountId } : {}),
  };
}

export function readCodexOfficialAuth(
  provider: Pick<Provider, "meta">,
): CodexOfficialAuthConfig {
  return normalizeCodexOfficialAuth(provider.meta?.codexOfficialAuth);
}

export function writeCodexOfficialAuth(
  provider: Provider,
  value: CodexOfficialAuthConfig,
): Provider {
  return {
    ...provider,
    meta: {
      ...provider.meta,
      codexOfficialAuth: normalizeCodexOfficialAuth(value),
    },
  };
}
