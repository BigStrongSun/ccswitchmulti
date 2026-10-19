import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CodexOfficialContextRow } from "./CodexOfficialContextRow";

describe("CodexOfficialContextRow", () => {
  it("does not offer the documented default above an unknown or lower limit", () => {
    const { rerender } = render(
      <CodexOfficialContextRow
        model="gpt-6.1-sol"
        value=""
        discovered={{
          id: "gpt-6.1-sol",
          ownedBy: "openai",
          contextWindow: 272000,
          maxContextWindow: 400000,
        }}
        onChange={vi.fn()}
      />,
    );

    let row = screen.getByRole("group", { name: "gpt-6.1-sol" });
    expect(within(row).getAllByText("500,000").length).toBeGreaterThan(0);
    expect(within(row).getByPlaceholderText("272,000")).toBeInTheDocument();
    expect(
      within(row).queryByRole("option", { name: /500,000/ }),
    ).not.toBeInTheDocument();

    rerender(
      <CodexOfficialContextRow
        model="gpt-6.1-sol"
        value=""
        discovered={{
          id: "gpt-6.1-sol",
          ownedBy: "openai",
          contextWindow: 272000,
        }}
        onChange={vi.fn()}
      />,
    );
    row = screen.getByRole("group", { name: "gpt-6.1-sol" });
    expect(
      within(row).queryByRole("option", { name: /500,000/ }),
    ).not.toBeInTheDocument();
  });

  it("does not duplicate the official preset when it equals the maximum", () => {
    render(
      <CodexOfficialContextRow
        model="gpt-6.1-sol"
        value=""
        discovered={{
          id: "gpt-6.1-sol",
          ownedBy: "openai",
          contextWindow: 500000,
          maxContextWindow: 500000,
        }}
        onChange={vi.fn()}
      />,
    );

    const row = screen.getByRole("group", { name: "gpt-6.1-sol" });
    expect(
      within(row)
        .getByRole("combobox")
        .querySelectorAll('option[value="500000"]'),
    ).toHaveLength(1);
  });
});
