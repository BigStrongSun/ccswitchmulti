import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { Form } from "@/components/ui/form";
import {
  ProviderPresetSelector,
  type PresetEntry,
} from "./ProviderPresetSelector";

const entries = ["Alpha", "Beta", "Gamma"].map(
  (name, index) =>
    ({
      id: `preset-${index}`,
      preset: { name, category: "custom" },
    }) as PresetEntry,
);

function SelectorFixture(
  props: React.ComponentProps<typeof ProviderPresetSelector>,
) {
  const form = useForm();
  return (
    <Form {...form}>
      <ProviderPresetSelector {...props} />
    </Form>
  );
}

describe("ProviderPresetSelector 折叠模式", () => {
  it("默认只展示限定数量，展开后展示全部预设", async () => {
    const user = userEvent.setup();
    render(
      <SelectorFixture
        selectedPresetId="custom"
        presetEntries={entries}
        presetCategoryLabels={{ custom: "Custom" }}
        onPresetChange={vi.fn()}
        collapsible
        initialVisibleCount={2}
      />,
    );

    expect(screen.getByRole("button", { name: "Alpha" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Beta" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Gamma" })).toBeNull();

    await user.click(screen.getByRole("button", { name: /展开全部/ }));
    expect(screen.getByRole("button", { name: "Gamma" })).toBeVisible();
  });

  it("选择预设后通知调用方滚动到后续表单", async () => {
    const user = userEvent.setup();
    const onPresetChange = vi.fn();
    const onPresetSelected = vi.fn();
    render(
      <SelectorFixture
        selectedPresetId="custom"
        presetEntries={entries}
        presetCategoryLabels={{ custom: "Custom" }}
        onPresetChange={onPresetChange}
        onPresetSelected={onPresetSelected}
        collapsible
        initialVisibleCount={2}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Alpha" }));

    expect(onPresetChange).toHaveBeenCalledWith("preset-0");
    expect(onPresetSelected).toHaveBeenCalledTimes(1);
  });
});
