import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const css = await readFile(new URL("./style.css", import.meta.url), "utf8");

assert.equal(
  css.includes("--surface-1"),
  false,
  "课件主题只定义 --surface；使用不存在的 --surface-1 会让背景声明失效并露出网格",
);

for (const selector of [
  ".plain-language",
  ".card,.tpl-document-deck .figure-box,.tpl-document-deck .table-card,.tpl-document-deck .code-box",
  ".conclusion",
  ".event",
]) {
  const rule = css.split(`.tpl-document-deck ${selector}`)[1]?.split("}")[0] ?? "";
  assert.match(rule, /background:[^;}]*var\(--surface\)/, `${selector} 必须使用主题的不透明表面色`);
  assert.doesNotMatch(rule, /background:[^;}]*transparent/, `${selector} 不能让页面网格透过正文表面`);
}

console.log("opaque content surfaces: ok");
