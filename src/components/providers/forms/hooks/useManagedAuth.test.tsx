import React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ManagedAuthDeviceCodeResponse,
  ManagedAuthStatus,
} from "@/lib/api";
import { useManagedAuth } from "./useManagedAuth";

const mocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authStartLogin: vi.fn(),
  authPollForAccount: vi.fn(),
  authCancelLogin: vi.fn(),
  openExternal: vi.fn(),
  copyText: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: mocks.authGetStatus,
    authStartLogin: mocks.authStartLogin,
    authPollForAccount: mocks.authPollForAccount,
    authCancelLogin: mocks.authCancelLogin,
  },
  settingsApi: { openExternal: mocks.openExternal },
}));

vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));

function renderManagedAuth() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: React.PropsWithChildren) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  return {
    ...renderHook(() => useManagedAuth("codex_oauth"), { wrapper }),
    queryClient,
  };
}

const loggedOutStatus: ManagedAuthStatus = {
  provider: "codex_oauth" as const,
  authenticated: false,
  default_account_id: null,
  accounts: [],
};

const loggedInStatus: ManagedAuthStatus = {
  provider: "codex_oauth" as const,
  authenticated: true,
  default_account_id: "account-1",
  accounts: [
    {
      id: "account-1",
      provider: "codex_oauth",
      login: "user@example.com",
      avatar_url: null,
      authenticated_at: 1,
      is_default: true,
      github_domain: "",
      requires_reauth: false,
    },
  ],
};

const deviceCode: ManagedAuthDeviceCodeResponse = {
  provider: "codex_oauth",
  device_code: "device-secret",
  user_code: "ABCD-EFGH",
  verification_uri: "https://example.com/device",
  expires_in: 1,
  interval: 1,
};

const legacyReauthStatus: ManagedAuthStatus = {
  provider: "codex_oauth",
  authenticated: false,
  default_account_id: null,
  accounts: [
    {
      id: "legacy-local-id",
      provider: "codex_oauth",
      login: "legacy@example.test",
      avatar_url: null,
      authenticated_at: 1,
      is_default: false,
      github_domain: "github.com",
      requires_reauth: true,
    },
  ],
};

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  mocks.authGetStatus.mockResolvedValue(loggedOutStatus);
  mocks.authStartLogin.mockResolvedValue(deviceCode);
  mocks.authPollForAccount.mockResolvedValue(null);
  mocks.authCancelLogin.mockResolvedValue(true);
  mocks.openExternal.mockResolvedValue(undefined);
  mocks.copyText.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useManagedAuth device flow", () => {
  it("同一登录请求尚未完成时忽略重复点击", async () => {
    let resolveLogin: ((value: typeof deviceCode) => void) | undefined;
    mocks.authStartLogin.mockReturnValue(
      new Promise((resolve) => {
        resolveLogin = resolve;
      }),
    );
    const { result } = renderManagedAuth();
    await waitFor(() =>
      expect(result.current.authStatus).toEqual(loggedOutStatus),
    );

    act(() => {
      result.current.startAuth();
      result.current.startAuth();
    });

    await waitFor(() => expect(mocks.authStartLogin).toHaveBeenCalled());
    expect(mocks.authStartLogin).toHaveBeenCalledTimes(1);
    await act(async () => resolveLogin?.(deviceCode));
    await waitFor(() => expect(result.current.pollingState).toBe("polling"));
    act(() => result.current.cancelAuth());
  });

  it("device code 到期时若账号已落盘则结束流程而不显示过期错误", async () => {
    let authoritativeStatus = loggedOutStatus;
    mocks.authGetStatus.mockImplementation(async () => authoritativeStatus);
    const { result } = renderManagedAuth();
    await waitFor(() =>
      expect(result.current.authStatus).toEqual(loggedOutStatus),
    );

    act(() => result.current.startAuth());
    await waitFor(() => expect(result.current.pollingState).toBe("polling"));

    authoritativeStatus = loggedInStatus;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_100);
    });

    await waitFor(() => expect(result.current.pollingState).toBe("idle"));
    expect(result.current.error).toBeNull();
    expect(result.current.hasAnyAccount).toBe(true);
  });

  it("upstream_codex_login_cancel notifies the backend for an active Codex flow", async () => {
    const { result } = renderManagedAuth();
    await waitFor(() =>
      expect(result.current.authStatus).toEqual(loggedOutStatus),
    );

    act(() => result.current.startAuth());
    await waitFor(() => expect(result.current.pollingState).toBe("polling"));
    act(() => result.current.cancelAuth());

    await waitFor(() =>
      expect(mocks.authCancelLogin).toHaveBeenCalledWith(
        "codex_oauth",
        "device-secret",
      ),
    );
  });

  it("upstream_codex_identity reauth keeps the existing local binding id", async () => {
    mocks.authGetStatus.mockResolvedValue(legacyReauthStatus);
    const { result } = renderManagedAuth();
    await waitFor(() =>
      expect(result.current.authStatus).toEqual(legacyReauthStatus),
    );

    act(() => result.current.reauthAccount("legacy-local-id"));

    await waitFor(() =>
      expect(mocks.authStartLogin).toHaveBeenCalledWith(
        "codex_oauth",
        undefined,
        "legacy-local-id",
      ),
    );
  });

  it("successful targeted reauthentication invalidates the stale account quota", async () => {
    mocks.authGetStatus
      .mockResolvedValueOnce(legacyReauthStatus)
      .mockResolvedValue(loggedInStatus);
    mocks.authPollForAccount.mockResolvedValue(loggedInStatus.accounts[0]);
    const { result, queryClient } = renderManagedAuth();
    const quotaKey = ["codex_oauth", "quota", "legacy-local-id"];
    queryClient.setQueryData(quotaKey, { credentialStatus: "reauth_required" });
    await waitFor(() =>
      expect(result.current.authStatus).toEqual(legacyReauthStatus),
    );

    act(() => result.current.reauthAccount("legacy-local-id"));

    await waitFor(() => expect(result.current.pollingState).toBe("idle"));
    expect(queryClient.getQueryState(quotaKey)?.isInvalidated).toBe(true);
  });
});
