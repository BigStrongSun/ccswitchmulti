import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import type { UniversalProvider } from "@/types";

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => <pre>{value}</pre>,
}));

const editingProvider: UniversalProvider = {
  id: "universal-existing",
  name: "Existing Gateway",
  providerType: "newapi",
  baseUrl: "https://gateway.example/v1",
  apiKey: "secret-key",
  apps: { claude: true, codex: true, gemini: true },
  models: {
    codex: { model: "qwen-visible", reasoningEffort: "medium" },
  },
};

function syncDialog() {
  return screen.getByRole("dialog", { name: "同步统一供应商" });
}

describe("UniversalProviderFormModal async persistence", () => {
  it("keeps the edited form and confirmation open when save-and-sync rejects", async () => {
    const rejected = Promise.reject(new Error("atomic save failed"));
    rejected.catch(() => undefined);
    const onSaveAndSync = vi.fn(() => rejected);
    const onClose = vi.fn();

    render(
      <UniversalProviderFormModal
        isOpen
        editingProvider={editingProvider}
        onSaveAndSync={onSaveAndSync}
        onClose={onClose}
      />,
    );

    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "https://changed.example/v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存并同步" }));
    fireEvent.click(
      within(syncDialog()).getByRole("button", { name: "保存并同步" }),
    );

    await waitFor(() => expect(onSaveAndSync).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(
        within(syncDialog()).getByRole("button", { name: "保存并同步" }),
      ).toBeEnabled(),
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("API 地址")).toHaveValue(
      "https://changed.example/v1",
    );
  });

  it("disables the confirmation and prevents duplicate writes while saving", async () => {
    let resolveSave!: () => void;
    const pendingSave = new Promise<void>((resolve) => {
      resolveSave = resolve;
    });
    const onSaveAndSync = vi.fn(() => pendingSave);
    const onClose = vi.fn();

    render(
      <UniversalProviderFormModal
        isOpen
        editingProvider={editingProvider}
        onSaveAndSync={onSaveAndSync}
        onClose={onClose}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存并同步" }));
    const confirm = within(syncDialog()).getByRole("button", {
      name: "保存并同步",
    });
    fireEvent.click(confirm);

    await waitFor(() => expect(confirm).toBeDisabled());
    fireEvent.click(confirm);
    expect(onSaveAndSync).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    resolveSave();
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("uses the same save-and-sync action for a new provider and keeps its draft after failure", async () => {
    const rejected = Promise.reject(new Error("save failed"));
    rejected.catch(() => undefined);
    const onSaveAndSync = vi.fn(() => rejected);
    const onClose = vi.fn();

    render(
      <UniversalProviderFormModal
        isOpen
        onSaveAndSync={onSaveAndSync}
        onClose={onClose}
      />,
    );

    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "https://new.example/v1" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "new-secret" },
    });
    expect(screen.queryByRole("button", { name: "添加" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "保存并同步" }));
    fireEvent.click(
      within(syncDialog()).getByRole("button", { name: "保存并同步" }),
    );

    await waitFor(() => expect(onSaveAndSync).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(
        within(syncDialog()).getByRole("button", { name: "保存并同步" }),
      ).toBeEnabled(),
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("API 地址")).toHaveValue(
      "https://new.example/v1",
    );
    expect(screen.getByLabelText("API Key")).toHaveValue("new-secret");
  });
});
