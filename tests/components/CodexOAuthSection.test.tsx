import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";

const mocks = vi.hoisted(() => ({
  useCodexOauth: vi.fn(),
  renderAccountQuota: vi.fn(),
}));

vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: mocks.useCodexOauth,
}));

vi.mock("@/components/CodexOauthAccountQuota", () => ({
  default: ({ accountId }: { accountId: string }) => {
    mocks.renderAccountQuota(accountId);
    return <div data-testid="account-quota">{accountId}</div>;
  },
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div />,
}));

vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div />,
}));

describe("CodexOAuthSection", () => {
  const renderWithQueryClient = (ui: ReactElement) => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    return render(
      <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
    );
  };

  beforeEach(() => {
    mocks.useCodexOauth.mockReturnValue({
      accounts: [
        {
          id: "account-1",
          provider: "codex_oauth",
          login: "user@example.com",
          avatar_url: null,
          authenticated_at: 0,
          is_default: true,
          github_domain: "",
        },
      ],
      defaultAccountId: "account-1",
      hasAnyAccount: true,
      pollingState: "idle",
      deviceCode: null,
      error: null,
      isPolling: false,
      isAddingAccount: false,
      isRemovingAccount: false,
      isSettingDefaultAccount: false,
      addAccount: vi.fn(),
      removeAccount: vi.fn(),
      setDefaultAccount: vi.fn(),
      cancelAuth: vi.fn(),
      logout: vi.fn(),
    });
  });

  it("does not render account quota by default", () => {
    renderWithQueryClient(<CodexOAuthSection />);

    expect(mocks.renderAccountQuota).not.toHaveBeenCalled();
    expect(screen.queryByTestId("account-quota")).not.toBeInTheDocument();
  });

  it("renders account quota in Auth Center", () => {
    renderWithQueryClient(<AuthCenterPanel />);

    expect(mocks.renderAccountQuota).toHaveBeenCalledWith("account-1");
    expect(screen.getByTestId("account-quota")).toHaveTextContent("account-1");
  });
});
