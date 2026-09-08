import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SubscriptionQuotaView } from "./SubscriptionQuotaFooter";
import type { SubscriptionQuota } from "@/types/subscription";

const identityUpgradeQuota: SubscriptionQuota = {
  tool: "codex_oauth",
  credentialStatus: "reauth_required",
  credentialMessage: "backend identity diagnostic",
  success: false,
  tiers: [],
  extraUsage: null,
  resetCredits: null,
  resetCreditsError: null,
  error: null,
  queriedAt: 1_788_793_200_000,
};

describe("SubscriptionQuotaView credential recovery", () => {
  it("offers targeted reauthentication instead of a no-op quota refresh", async () => {
    const user = userEvent.setup();
    const refetch = vi.fn();
    const reauthenticate = vi.fn();

    render(
      <SubscriptionQuotaView
        quota={identityUpgradeQuota}
        loading={false}
        refetch={refetch}
        onReauthenticate={reauthenticate}
        appIdForExpiredHint="codex_oauth"
      />,
    );

    expect(
      screen.getByText("旧账号缺少新版身份信息，需要重新认证"),
    ).toBeVisible();
    expect(screen.queryByTitle("subscription.refresh")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重新认证" }));
    expect(reauthenticate).toHaveBeenCalledTimes(1);
    expect(refetch).not.toHaveBeenCalled();
  });

  it("shows that targeted reauthentication is already in progress", () => {
    render(
      <SubscriptionQuotaView
        quota={identityUpgradeQuota}
        loading={false}
        refetch={vi.fn()}
        onReauthenticate={vi.fn()}
        reauthenticating={true}
        appIdForExpiredHint="codex_oauth"
      />,
    );

    expect(screen.getByRole("button", { name: "正在重新认证" })).toBeDisabled();
  });
});
