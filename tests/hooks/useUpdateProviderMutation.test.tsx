import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useUpdateProviderMutation } from "@/lib/query/mutations";
import { usageKeys } from "@/lib/query/usage";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  update: vi.fn(),
  getAll: vi.fn(),
  inspectActiveCodexMultiRouterProjection: vi.fn(),
}));

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    update: (...args: unknown[]) => apiMocks.update(...args),
    getAll: (...args: unknown[]) => apiMocks.getAll(...args),
    inspectActiveCodexMultiRouterProjection: (...args: unknown[]) =>
      apiMocks.inspectActiveCodexMultiRouterProjection(...args),
  },
  sessionsApi: {},
  settingsApi: {},
}));

vi.mock("@/hooks/useHermes", () => ({
  invalidateHermesProviderCaches: vi.fn(),
}));

vi.mock("@/hooks/useOpenClaw", () => ({
  openclawKeys: {
    health: ["openclaw", "health"],
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("sonner", () => ({
  toast: toastMocks,
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, invalidateSpy };
}

function createProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: "provider-1",
    name: "Test Provider",
    settingsConfig: {},
    ...overrides,
  };
}

beforeEach(() => {
  apiMocks.update.mockReset().mockResolvedValue(true);
  apiMocks.getAll.mockReset().mockResolvedValue({});
  apiMocks.inspectActiveCodexMultiRouterProjection
    .mockReset()
    .mockResolvedValue({
      routerProviderId: "router-active",
      state: "ready",
    });
  toastMocks.success.mockReset();
  toastMocks.error.mockReset();
  toastMocks.warning.mockReset();
});

describe("useUpdateProviderMutation", () => {
  it("invalidates the updated provider usage query", async () => {
    const { wrapper, invalidateSpy } = createWrapper();
    const provider = createProvider({ id: "provider-b" });
    const { result } = renderHook(() => useUpdateProviderMutation("codex"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({ provider });
    });

    expect(apiMocks.update).toHaveBeenCalledWith(provider, "codex", undefined);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["providers", "codex"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: usageKeys.script("provider-b", "codex"),
    });
    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: usageKeys.all,
    });
  });

  it("submits one Codex Provider mutation without rewriting MultiRouter plans in the frontend", async () => {
    const { wrapper } = createWrapper();
    const provider = createProvider({
      id: "provider-b",
      settingsConfig: {
        modelCatalog: { models: [{ model: "new-model" }] },
      },
    });
    const { result } = renderHook(() => useUpdateProviderMutation("codex"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({ provider });
    });

    expect(apiMocks.getAll).not.toHaveBeenCalled();
    expect(apiMocks.update).toHaveBeenCalledTimes(1);
    expect(apiMocks.update).toHaveBeenCalledWith(provider, "codex", undefined);
  });

  it("warns immediately when the active MultiRouter projection is pending after save", async () => {
    apiMocks.inspectActiveCodexMultiRouterProjection.mockResolvedValueOnce({
      routerProviderId: "router-active",
      state: "pending",
      lastErrorCode: "projection_live_drift",
      lastErrorMessage: "live files differ",
    });
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useUpdateProviderMutation("codex"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({ provider: createProvider() });
    });

    expect(
      apiMocks.inspectActiveCodexMultiRouterProjection,
    ).toHaveBeenCalledTimes(1);
    expect(toastMocks.warning).toHaveBeenCalledWith(
      "Provider 已保存，但当前 MultiRouter 尚未同步到 Codex。请到 MultiRouter 工作台查看并重试。",
      { closeButton: true },
    );
  });

  it("also invalidates the previous usage query when provider id changes", async () => {
    const { wrapper, invalidateSpy } = createWrapper();
    const provider = createProvider({ id: "provider-new" });
    const { result } = renderHook(() => useUpdateProviderMutation("openclaw"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({
        provider,
        originalId: "provider-old",
      });
    });

    expect(apiMocks.update).toHaveBeenCalledWith(
      provider,
      "openclaw",
      "provider-old",
    );
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: usageKeys.script("provider-new", "openclaw"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: usageKeys.script("provider-old", "openclaw"),
    });
    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: usageKeys.all,
    });
  });
});
