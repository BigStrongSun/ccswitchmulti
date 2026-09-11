import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ManagedAuthAccount } from "@/lib/api/auth";
import {
  CodexOfficialAuthSection,
  validateCodexOfficialAuthSelection,
} from "./CodexOfficialAuthSection";

const accounts: ManagedAuthAccount[] = [
  {
    id: "account-ready",
    provider: "codex_oauth",
    login: "ready@example.com",
    avatar_url: null,
    authenticated_at: 1,
    is_default: true,
    github_domain: "github.com",
    requires_reauth: false,
  },
  {
    id: "account-expired",
    provider: "codex_oauth",
    login: "expired@example.com",
    avatar_url: null,
    authenticated_at: 1,
    is_default: false,
    github_domain: "github.com",
    requires_reauth: true,
  },
];

describe("CodexOfficialAuthSection", () => {
  it("offers all three Provider-owned official authentication modes", () => {
    render(
      <CodexOfficialAuthSection
        value={{ mode: "desktop_current_login" }}
        accounts={accounts}
        onChange={vi.fn()}
        onOpenAuthCenter={vi.fn()}
      />,
    );

    const mode = screen.getByRole("combobox", { name: "官方认证方式" });
    expect(mode).toHaveValue("desktop_current_login");
    expect(
      screen.getByRole("option", { name: "Codex Desktop 当前登录" }),
    ).toBeVisible();
    expect(
      screen.getByRole("option", { name: "CCSM OAuth 固定账号" }),
    ).toBeVisible();
    expect(screen.getByRole("option", { name: "OAuth 账号池" })).toBeVisible();
  });

  it("returns a normalized fixed account selection", () => {
    const onChange = vi.fn();
    render(
      <CodexOfficialAuthSection
        value={{ mode: "managed_oauth" }}
        accounts={accounts}
        onChange={onChange}
        onOpenAuthCenter={vi.fn()}
      />,
    );

    fireEvent.change(
      screen.getByRole("combobox", { name: "CCSM OAuth 固定账号" }),
      { target: { value: "account-ready" } },
    );
    expect(onChange).toHaveBeenCalledWith({
      mode: "managed_oauth",
      accountId: "account-ready",
    });
  });

  it("rejects a missing or reauthentication-required fixed account", () => {
    expect(
      validateCodexOfficialAuthSelection(
        { mode: "managed_oauth", accountId: "missing" },
        accounts,
      ),
    ).toBe("所选 CCSM OAuth 账号不存在或需要重新登录");
    expect(
      validateCodexOfficialAuthSelection(
        { mode: "managed_oauth", accountId: "account-expired" },
        accounts,
      ),
    ).toBe("所选 CCSM OAuth 账号不存在或需要重新登录");
    expect(
      validateCodexOfficialAuthSelection(
        { mode: "managed_oauth", accountId: "account-ready" },
        accounts,
      ),
    ).toBeNull();
  });

  it("shows conflicts without hiding the account-pool settings link", () => {
    const onOpenAuthCenter = vi.fn();
    render(
      <CodexOfficialAuthSection
        value={{ mode: "account_pool" }}
        accounts={accounts}
        migrationStatus={{
          state: "conflict",
          conflictingRouterIds: ["router-native", "router-pool"],
        }}
        onChange={vi.fn()}
        onOpenAuthCenter={onOpenAuthCenter}
      />,
    );

    expect(screen.getByText(/router-native、router-pool/)).toBeVisible();
    expect(screen.getByText(/无需启用 MultiRouter/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "前往认证中心" }));
    expect(onOpenAuthCenter).toHaveBeenCalledTimes(1);
  });
});
