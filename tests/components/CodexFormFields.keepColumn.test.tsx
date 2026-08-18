import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";
import type { CodexCatalogModel } from "@/types";

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

function renderCodexFormFields(catalogModels: CodexCatalogModel[]) {
  const onCatalogModelsChange = vi.fn();
  render(
    <FormShell>
      <CodexFormFields
        codexApiKey="sk-test"
        onApiKeyChange={vi.fn()}
        websiteUrl="https://example.com"
        shouldShowApiKeyLink
        shouldShowSpeedTest={false}
        codexBaseUrl="https://api.example.com"
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        apiFormat="openai_chat"
        onApiFormatChange={vi.fn()}
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
        catalogModels={catalogModels}
        onCatalogModelsChange={onCatalogModelsChange}
      />
    </FormShell>,
  );
  return { onCatalogModelsChange };
}

describe("CodexFormFields enable column", () => {
  it("keeps the catalog row and disables the model when enable is unchecked", async () => {
    const { onCatalogModelsChange } = renderCodexFormFields([
      { model: "deepseek-v4-flash" },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

    expect(screen.getAllByText("启用").length).toBeGreaterThan(0);
    const enableCheckbox = screen.getByRole("checkbox", {
      name: "启用 deepseek-v4-flash",
    });
    expect(enableCheckbox).toBeChecked();

    fireEvent.click(enableCheckbox);

    await waitFor(() => {
      expect(enableCheckbox).not.toBeChecked();
      expect(screen.getByText("未启用")).toBeInTheDocument();
    });
    expect(
      screen.getAllByDisplayValue("deepseek-v4-flash").length,
    ).toBeGreaterThan(0);
    expect(onCatalogModelsChange).toHaveBeenCalledWith([
      expect.objectContaining({
        model: "deepseek-v4-flash",
        enabled: false,
      }),
    ]);
  });

  it("re-enables the model when the disabled row is checked again", async () => {
    const { onCatalogModelsChange } = renderCodexFormFields([
      { model: "deepseek-v4-flash", enabled: false },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));

    const enableCheckbox = screen.getByRole("checkbox", {
      name: "启用 deepseek-v4-flash",
    });
    expect(enableCheckbox).not.toBeChecked();
    expect(screen.getByText("未启用")).toBeInTheDocument();

    fireEvent.click(enableCheckbox);

    await waitFor(() => {
      expect(enableCheckbox).toBeChecked();
      expect(screen.queryByText("未启用")).not.toBeInTheDocument();
    });
    expect(onCatalogModelsChange).toHaveBeenLastCalledWith([
      expect.objectContaining({
        model: "deepseek-v4-flash",
        enabled: true,
      }),
    ]);
  });
});
