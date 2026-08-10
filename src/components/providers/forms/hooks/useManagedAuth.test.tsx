import React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useManagedAuth } from "./useManagedAuth";

const mocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authStartLogin: vi.fn(),
  authPollForAccount: vi.fn(),
  openExternal: vi.fn(),
  copyText: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: mocks.authGetStatus,
    authStartLogin: mocks.authStartLogin,
    authPollForAccount: mocks.authPollForAccount,
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
  return renderHook(() => useManagedAuth("codex_oauth"), { wrapper });
}

const loggedOutStatus = {
  provider: "codex_oauth" as const,
  authenticated: false,
  default_account_id: null,
  accounts: [],
};

const loggedInStatus = {
  provider: "codex_oauth" as const,
  authenticated: true,
  default_account_id: "account-1",
  accounts: [{ id: "account-1", login: "user@example.com" }],
};

const deviceCode = {
  device_code: "device-secret",
  user_code: "ABCD-EFGH",
  verification_uri: "https://example.com/device",
  expires_in: 1,
  interval: 1,
};

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  mocks.authGetStatus.mockResolvedValue(loggedOutStatus);
  mocks.authStartLogin.mockResolvedValue(deviceCode);
  mocks.authPollForAccount.mockResolvedValue(null);
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
    await waitFor(() => expect(result.current.authStatus).toEqual(loggedOutStatus));

    act(() => {
      result.current.startAuth();
      result.current.startAuth();
    });

    await waitFor(() => expect(mocks.authStartLogin).toHaveBeenCalled());
    expect(mocks.authStartLogin).toHaveBeenCalledTimes(1);
    resolveLogin?.(deviceCode);
  });

  it("device code 到期时若账号已落盘则结束流程而不显示过期错误", async () => {
    let authoritativeStatus = loggedOutStatus;
    mocks.authGetStatus.mockImplementation(async () => authoritativeStatus);
    const { result } = renderManagedAuth();
    await waitFor(() => expect(result.current.authStatus).toEqual(loggedOutStatus));

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
});
