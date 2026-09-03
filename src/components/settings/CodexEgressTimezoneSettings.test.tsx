import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { codexEgressTimezoneApi } from "@/lib/api/codexEgressTimezone";
import { CodexEgressTimezoneSettings } from "./CodexEgressTimezoneSettings";

vi.mock("@/lib/api/codexEgressTimezone", () => ({
  codexEgressTimezoneApi: {
    detect: vi.fn(),
    inspectRuntime: vi.fn(),
    validate: vi.fn(),
  },
}));

describe("CodexEgressTimezoneSettings", () => {
  beforeEach(() => {
    vi.mocked(codexEgressTimezoneApi.detect).mockReset();
    vi.mocked(codexEgressTimezoneApi.inspectRuntime).mockReset();
    vi.mocked(codexEgressTimezoneApi.validate).mockReset();
    vi.mocked(codexEgressTimezoneApi.validate).mockResolvedValue(undefined);
  });

  it("detects the real ChatGPT egress behind fake-IP DNS and lets the user opt in", async () => {
    vi.mocked(codexEgressTimezoneApi.detect).mockResolvedValue({
      targetHost: "chatgpt.com",
      dnsAddresses: ["198.18.0.14"],
      dnsUsesNonPublicAddress: true,
      egressIp: "2407:cdc0:…",
      countryCode: "TW",
      region: "Taipei",
      city: "Taipei",
      colo: "TPE",
      egressTimezone: "Asia/Taipei",
      currentTimezone: "Asia/Shanghai",
      egressUtcOffset: "+08:00",
      currentUtcOffset: "+08:00",
      timezoneMatch: "offset_match",
      checkedAt: 1_787_875_200,
      networkPath: "system_or_transparent",
    });
    const onChange = vi.fn().mockResolvedValue(true);

    render(
      <CodexEgressTimezoneSettings
        value={{ mode: "off" }}
        onChange={onChange}
      />,
    );

    expect(screen.getByText("Codex 出口时区（实验）")).toBeInTheDocument();
    expect(screen.getByText(/不会修改 Windows 系统时区/)).toBeInTheDocument();
    expect(
      screen.getByText(/没有官方证据证明时区不一致一定导致模型降级/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "探测出口时区" }));

    expect(await screen.findByText("Asia/Taipei")).toBeInTheDocument();
    expect(screen.getByText("Asia/Shanghai")).toBeInTheDocument();
    expect(
      screen.getByText(/IANA 名称不同，但当前 UTC 偏移相同/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/DNS 返回 198\.18\.0\.14.*fake-IP.*不会拿它做地理定位/),
    ).toBeInTheDocument();
    expect(screen.getByText(/2407:cdc0:…/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "使用探测结果" }));
    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "auto",
          detectedTimezone: "Asia/Taipei",
          detectedEgressIp: "2407:cdc0:…",
          detectedCountryCode: "TW",
        }),
      ),
    );
  });

  it("supports an explicit IANA timezone override without touching the system timezone", async () => {
    const onChange = vi.fn().mockResolvedValue(true);
    render(
      <CodexEgressTimezoneSettings
        value={{ mode: "off" }}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "手动设置" }));
    fireEvent.change(screen.getByLabelText("IANA 时区"), {
      target: { value: "America/Los_Angeles" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存手动时区" }));

    await waitFor(() =>
      expect(codexEgressTimezoneApi.validate).toHaveBeenCalledWith(
        "America/Los_Angeles",
      ),
    );
    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "manual",
          manualTimezone: "America/Los_Angeles",
        }),
      ),
    );
  });

  it("rejects an unknown IANA timezone before settings are saved", async () => {
    vi.mocked(codexEgressTimezoneApi.validate).mockRejectedValue(
      new Error("未知的 IANA 时区: America/Fake"),
    );
    const onChange = vi.fn().mockResolvedValue(true);
    render(
      <CodexEgressTimezoneSettings
        value={{ mode: "off" }}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "手动设置" }));
    fireEvent.change(screen.getByLabelText("IANA 时区"), {
      target: { value: "America/Fake" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存手动时区" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "未知的 IANA 时区: America/Fake",
    );
    expect(onChange).not.toHaveBeenCalled();
  });

  it("verifies the timezone reported by the running Codex renderer", async () => {
    vi.mocked(codexEgressTimezoneApi.inspectRuntime).mockResolvedValue({
      runtimeTimezone: "Asia/Taipei",
      runtimeUtcOffset: "+08:00",
      configuredTimezone: "Asia/Taipei",
      matchesConfigured: true,
      timezoneMatch: "exact",
    });

    render(
      <CodexEgressTimezoneSettings
        value={{ mode: "auto", detectedTimezone: "Asia/Taipei" }}
        onChange={vi.fn()}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "检查运行中 Codex 页面" }),
    );

    expect(
      await screen.findByText(
        "运行中的 Codex renderer：Asia/Taipei (+08:00)，与当前配置一致。",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/app-server 只有在由 CCSM 直接拉起时才会继承 TZ/),
    ).toBeInTheDocument();
  });
});
