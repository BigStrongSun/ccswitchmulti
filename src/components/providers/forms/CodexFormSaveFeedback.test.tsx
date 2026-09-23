import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CodexFormSaveFeedback } from "./CodexFormFields";

describe("CodexFormSaveFeedback", () => {
  it("keeps catalog validation visible and explicitly says the whole draft was not saved", () => {
    render(
      <CodexFormSaveFeedback
        catalogValidationError={{
          model: "deepseek",
          message: "deepseek：Ultra 尚未选择供应商推理强度",
        }}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("Ultra 尚未选择");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "API Key、端点及其他修改仍保留在当前草稿中，但本次所有修改均未保存。",
    );
  });

  it("shows a generic save error without rendering the secret draft value", () => {
    render(
      <CodexFormSaveFeedback saveError="Codex 配置无效或无法解析，本次所有修改均未保存。" />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("所有修改均未保存");
    expect(screen.getByRole("alert")).not.toHaveTextContent("synthetic-secret");
  });
});
