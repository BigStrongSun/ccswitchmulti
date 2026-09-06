import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";

const mocks = vi.hoisted(() => ({
  useCodexOauth: vi.fn(),
  renderAccountQuota: vi.fn(),
  reauthAccount: vi.fn(),
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
          requires_reauth: false,
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
      reauthAccount: mocks.reauthAccount,
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

  it("upstream_codex_identity keeps a quarantined account visible for reauthentication", async () => {
    mocks.useCodexOauth.mockReturnValue({
      accounts: [
        {
          id: "legacy-local-id",
          provider: "codex_oauth",
          login: "legacy@example.test",
          avatar_url: null,
          authenticated_at: 0,
          is_default: false,
          github_domain: "github.com",
          requires_reauth: true,
        },
      ],
      defaultAccountId: null,
      hasAnyAccount: false,
      pollingState: "idle",
      deviceCode: null,
      error: null,
      authError: null,
      isPolling: false,
      isAddingAccount: false,
      isRemovingAccount: false,
      isSettingDefaultAccount: false,
      addAccount: vi.fn(),
      reauthAccount: mocks.reauthAccount,
      removeAccount: vi.fn(),
      setDefaultAccount: vi.fn(),
      cancelAuth: vi.fn(),
      logout: vi.fn(),
    });

    renderWithQueryClient(<CodexOAuthSection />);
    screen.getByRole("button", { name: "重新认证" }).click();

    expect(mocks.reauthAccount).toHaveBeenCalledWith("legacy-local-id");
  });

  it("upstream_codex_identity renders duplicate-login errors as localized guidance", () => {
    mocks.useCodexOauth.mockReturnValue({
      accounts: [],
      defaultAccountId: null,
      hasAnyAccount: false,
      pollingState: "error",
      deviceCode: null,
      error: "codex_oauth_duplicate_account",
      authError: null,
      isPolling: false,
      isAddingAccount: false,
      isRemovingAccount: false,
      isSettingDefaultAccount: false,
      addAccount: vi.fn(),
      reauthAccount: mocks.reauthAccount,
      retryAuth: vi.fn(),
      removeAccount: vi.fn(),
      setDefaultAccount: vi.fn(),
      cancelAuth: vi.fn(),
      logout: vi.fn(),
    });

    renderWithQueryClient(<CodexOAuthSection />);

    expect(
      screen.getByText("该 ChatGPT 账号已经存在，无需重复添加。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("codex_oauth_duplicate_account"),
    ).not.toBeInTheDocument();
  });
});
