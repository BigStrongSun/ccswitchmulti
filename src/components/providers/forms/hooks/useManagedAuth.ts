import { useState, useCallback, useRef, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { authApi, settingsApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import type {
  ManagedAuthProvider,
  ManagedAuthStatus,
  ManagedAuthDeviceCodeResponse,
} from "@/lib/api";

type PollingState = "idle" | "polling" | "success" | "error";

export function useManagedAuth(
  authProvider: ManagedAuthProvider,
  githubDomain?: string,
) {
  const queryClient = useQueryClient();
  const queryKey = ["managed-auth-status", authProvider];

  const [pollingState, setPollingState] = useState<PollingState>("idle");
  const [deviceCode, setDeviceCode] =
    useState<ManagedAuthDeviceCodeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const pollingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(
    null,
  );
  const pollingTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flowGenerationRef = useRef(0);
  const flowActiveRef = useRef(false);
  const expirationCheckRef = useRef(false);

  const {
    data: authStatus,
    isLoading: isLoadingStatus,
    refetch: refetchStatus,
  } = useQuery<ManagedAuthStatus>({
    queryKey,
    queryFn: () => authApi.authGetStatus(authProvider),
    staleTime: 30000,
    // A rejected xAI refresh token is persisted as `requires_reauth` by the
    // proxy hot path. Periodically refresh local status so an already-open Auth
    // Center stops showing the account as logged in without requiring a reload.
    refetchInterval: authProvider === "xai_oauth" ? 15_000 : false,
  });

  const stopPolling = useCallback(() => {
    if (pollingIntervalRef.current) {
      clearInterval(pollingIntervalRef.current);
      pollingIntervalRef.current = null;
    }
    if (pollingTimeoutRef.current) {
      clearTimeout(pollingTimeoutRef.current);
      pollingTimeoutRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => {
      flowGenerationRef.current += 1;
      flowActiveRef.current = false;
      stopPolling();
    };
  }, [stopPolling]);

  const finishFlow = useCallback(
    (flowGeneration: number) => {
      if (flowGeneration !== flowGenerationRef.current) return false;
      stopPolling();
      flowActiveRef.current = false;
      expirationCheckRef.current = false;
      flowGenerationRef.current += 1;
      return true;
    },
    [stopPolling],
  );

  const finishExpiredFlow = useCallback(
    async (flowGeneration: number) => {
      if (
        flowGeneration !== flowGenerationRef.current ||
        expirationCheckRef.current
      ) {
        return;
      }
      expirationCheckRef.current = true;
      stopPolling();

      try {
        const latest = await refetchStatus();
        if (flowGeneration !== flowGenerationRef.current) return;
        const status = latest.data;
        if (status?.authenticated && status.accounts.length > 0) {
          finishFlow(flowGeneration);
          setPollingState("idle");
          setDeviceCode(null);
          setError(null);
          await queryClient.invalidateQueries({ queryKey });
          return;
        }
      } catch (statusError) {
        console.debug(
          "[ManagedAuth] Failed to refresh status after device expiry:",
          statusError,
        );
      }

      if (flowGeneration === flowGenerationRef.current) {
        finishFlow(flowGeneration);
        setPollingState("error");
        setError("Device code expired. Please try again.");
      }
    },
    [finishFlow, queryClient, queryKey, refetchStatus, stopPolling],
  );

  const startLoginMutation = useMutation({
    mutationFn: async (flowGeneration: number) => ({
      flowGeneration,
      response: await authApi.authStartLogin(authProvider, githubDomain),
    }),
    onSuccess: async ({ flowGeneration, response }) => {
      if (flowGeneration !== flowGenerationRef.current) return;
      setDeviceCode(response);
      setPollingState("polling");
      setError(null);

      try {
        await copyText(response.user_code);
      } catch (e) {
        console.debug("[ManagedAuth] Failed to copy user code:", e);
      }

      try {
        await settingsApi.openExternal(response.verification_uri);
      } catch (e) {
        console.debug("[ManagedAuth] Failed to open browser:", e);
      }

      // Add a small buffer on top of GitHub's suggested interval to avoid
      // hitting slow_down responses too aggressively during device polling.
      const interval = Math.max((response.interval || 5) + 3, 8) * 1000;
      const expiresAt = Date.now() + response.expires_in * 1000;

      const pollOnce = async () => {
        if (flowGeneration !== flowGenerationRef.current) return;
        if (Date.now() > expiresAt) {
          await finishExpiredFlow(flowGeneration);
          return;
        }

        try {
          const newAccount = await authApi.authPollForAccount(
            authProvider,
            response.device_code,
            githubDomain,
          );
          if (newAccount) {
            if (!finishFlow(flowGeneration)) return;
            setPollingState("success");
            await refetchStatus();
            await queryClient.invalidateQueries({ queryKey });
            setPollingState("idle");
            setDeviceCode(null);
          }
        } catch (e) {
          if (flowGeneration !== flowGenerationRef.current) return;
          const errorMessage = e instanceof Error ? e.message : String(e);
          if (
            !errorMessage.includes("pending") &&
            !errorMessage.includes("slow_down")
          ) {
            stopPolling();
            setPollingState("error");
            setError(errorMessage);
          }
        }
      };

      void pollOnce();
      pollingIntervalRef.current = setInterval(pollOnce, interval);
      pollingTimeoutRef.current = setTimeout(() => {
        void finishExpiredFlow(flowGeneration);
      }, response.expires_in * 1000);
    },
    onError: (e, flowGeneration) => {
      if (!finishFlow(flowGeneration)) return;
      setPollingState("error");
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const logoutMutation = useMutation({
    mutationFn: () => authApi.authLogout(authProvider),
    onSuccess: async () => {
      setPollingState("idle");
      setDeviceCode(null);
      setError(null);
      queryClient.setQueryData(queryKey, {
        provider: authProvider,
        authenticated: false,
        default_account_id: null,
        accounts: [],
      });
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: async (e) => {
      console.error("[ManagedAuth] Failed to logout:", e);
      setError(e instanceof Error ? e.message : String(e));
      await refetchStatus();
    },
  });

  const removeAccountMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authRemoveAccount(authProvider, accountId),
    onSuccess: async () => {
      setPollingState("idle");
      setDeviceCode(null);
      setError(null);
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (e) => {
      console.error("[ManagedAuth] Failed to remove account:", e);
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const setDefaultAccountMutation = useMutation({
    mutationFn: (accountId: string) =>
      authApi.authSetDefaultAccount(authProvider, accountId),
    onSuccess: async () => {
      await refetchStatus();
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (e) => {
      console.error("[ManagedAuth] Failed to set default account:", e);
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  const startAuth = useCallback(() => {
    if (flowActiveRef.current) return;
    flowActiveRef.current = true;
    expirationCheckRef.current = false;
    const flowGeneration = flowGenerationRef.current + 1;
    flowGenerationRef.current = flowGeneration;
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
    stopPolling();
    startLoginMutation.mutate(flowGeneration);
  }, [startLoginMutation, stopPolling]);

  const cancelAuth = useCallback(() => {
    flowGenerationRef.current += 1;
    flowActiveRef.current = false;
    expirationCheckRef.current = false;
    stopPolling();
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
  }, [stopPolling]);

  const logout = useCallback(() => {
    logoutMutation.mutate();
  }, [logoutMutation]);

  const removeAccount = useCallback(
    (accountId: string) => {
      removeAccountMutation.mutate(accountId);
    },
    [removeAccountMutation],
  );

  const setDefaultAccount = useCallback(
    (accountId: string) => {
      setDefaultAccountMutation.mutate(accountId);
    },
    [setDefaultAccountMutation],
  );

  const accounts = authStatus?.accounts ?? [];
  // 账号列表只表示本地仍有记录，authenticated 才表示后端认为该认证状态可用；
  // Codex OAuth 在 refresh token 明确失效时会清理账号或降级状态，避免 UI 误报已登录。
  const hasUsableAccount =
    (authStatus?.authenticated ?? false) && accounts.length > 0;

  return {
    authStatus,
    isLoadingStatus,
    accounts,
    hasAnyAccount: hasUsableAccount,
    isAuthenticated: authStatus?.authenticated ?? false,
    defaultAccountId: authStatus?.default_account_id ?? null,
    migrationError: authStatus?.migration_error ?? null,
    authError: authStatus?.auth_error ?? null,
    pollingState,
    deviceCode,
    error,
    isPolling: pollingState === "polling",
    isAddingAccount: startLoginMutation.isPending || pollingState === "polling",
    isRemovingAccount: removeAccountMutation.isPending,
    isSettingDefaultAccount: setDefaultAccountMutation.isPending,
    startAuth,
    addAccount: startAuth,
    cancelAuth,
    logout,
    removeAccount,
    setDefaultAccount,
    refetchStatus,
  };
}
