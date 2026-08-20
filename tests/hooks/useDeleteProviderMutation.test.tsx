import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDeleteProviderMutation } from "@/lib/query/mutations";

const apiMocks = vi.hoisted(() => ({
  delete: vi.fn(),
  update: vi.fn(),
  getAll: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    delete: (...args: unknown[]) => apiMocks.delete(...args),
    update: (...args: unknown[]) => apiMocks.update(...args),
    getAll: (...args: unknown[]) => apiMocks.getAll(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
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
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper };
}

beforeEach(() => {
  apiMocks.delete.mockReset().mockResolvedValue(true);
  apiMocks.update.mockReset().mockResolvedValue(true);
  apiMocks.getAll.mockReset().mockResolvedValue({});
  apiMocks.updateTrayMenu.mockReset().mockResolvedValue(undefined);
});

describe("useDeleteProviderMutation", () => {
  it("删除 Codex 供应商交给后端 SSOT coordinator 做级联", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useDeleteProviderMutation("codex"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync("beta");
    });

    expect(apiMocks.delete).toHaveBeenCalledWith("beta", "codex");
    expect(apiMocks.getAll).not.toHaveBeenCalled();
    expect(apiMocks.update).not.toHaveBeenCalled();
  });

  it("非 Codex 应用删除供应商不触发 MultiRouter 同步", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useDeleteProviderMutation("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync("provider-1");
    });

    expect(apiMocks.delete).toHaveBeenCalledWith("provider-1", "claude");
    expect(apiMocks.getAll).not.toHaveBeenCalled();
    expect(apiMocks.update).not.toHaveBeenCalled();
  });
});
