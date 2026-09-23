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

  it("shows a generic pre-submit error without rendering the secret draft value", () => {
    render(
      <CodexFormSaveFeedback saveError="Codex 设置校验或解析失败；本次尚未提交，草稿仍保留。" />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("本次尚未提交");
    expect(screen.getByRole("alert")).not.toHaveTextContent("synthetic-secret");
  });

  it("does not claim a rejected backend save was not written", () => {
    render(
      <CodexFormSaveFeedback saveError="保存结果未确认。后端可能已写入，但后续同步失败；请刷新核对状态，核对前避免重复修改 API Key。当前草稿仍保留。" />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("保存结果未确认");
    expect(screen.getByRole("alert")).toHaveTextContent("后端可能已写入");
    expect(screen.getByRole("alert")).toHaveTextContent("刷新核对状态");
    expect(screen.getByRole("alert")).toHaveTextContent("避免重复修改 API Key");
  });

  it("shows canceled Codex saves as definitely not saved", () => {
    render(
      <CodexFormSaveFeedback saveError="操作已取消，未保存。当前草稿仍保留，可继续编辑后重试。" />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("操作已取消，未保存");
    expect(screen.getByRole("alert")).toHaveTextContent("当前草稿仍保留");
  });
});
