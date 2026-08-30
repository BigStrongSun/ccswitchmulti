import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: null }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: null }),
}));

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: () => null,
}));

vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/providers/ProviderHealthBadge", () => ({
  ProviderHealthBadge: () => null,
}));
vi.mock("@/components/providers/FailoverPriorityBadge", () => ({
  FailoverPriorityBadge: () => null,
}));

const callbacks = {
  onSwitch: vi.fn(),
  onEdit: vi.fn(),
  onDelete: vi.fn(),
  onConfigureUsage: vi.fn(),
  onOpenWebsite: vi.fn(),
  onDuplicate: vi.fn(),
};

function provider(settingsConfig: Provider["settingsConfig"]): Provider {
  return {
    id: "qwen",
    name: "Qwen",
    category: "custom",
    settingsConfig,
  };
}

describe("ProviderCard Codex adaptation marker", () => {
  it("shows an automatic adaptation marker for a generated facade instead of MultiRouter", () => {
    render(
      <ProviderCard
        provider={provider({
          codexProtocolSet: { role: "facade" },
          codexRouting: { enabled: true, routes: [{ id: "responses" }] },
        })}
        adaptationSummary={{
          providerId: "qwen",
          persistence: "split",
          status: "ready",
          effectiveTransport: "mixed",
          modelCount: 2,
        }}
        isCurrent={false}
        appId="codex"
        isProxyRunning
        {...callbacks}
      />,
    );

    expect(
      screen.getByText("自动适配 · Responses + Chat"),
    ).toBeInTheDocument();
    expect(screen.queryByText("MultiRouter")).not.toBeInTheDocument();
  });

  it("keeps a user MultiRouter marked as MultiRouter even when it has an adaptation summary", () => {
    render(
      <ProviderCard
        provider={provider({
          codexRouting: { schemaVersion: 2, enabled: true, routes: [] },
        })}
        adaptationSummary={{
          providerId: "qwen",
          persistence: "single",
          status: "not_tested",
          effectiveTransport: null,
          modelCount: 0,
        }}
        isCurrent={false}
        appId="codex"
        isProxyRunning
        {...callbacks}
      />,
    );

    expect(screen.getByText("MultiRouter")).toBeInTheDocument();
    expect(
      screen.queryByText("自动适配 · Responses + Chat"),
    ).not.toBeInTheDocument();
  });
});
