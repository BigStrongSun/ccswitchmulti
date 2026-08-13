import fs from "node:fs";
import path from "node:path";

const dir = path.dirname(new URL(import.meta.url).pathname.replace(/^\/(.:)/, "$1"));
const html = fs.readFileSync(path.join(dir, "index.html"), "utf8");
const js = fs.readFileSync(path.join(dir, "deck.js"), "utf8");
const catalog = JSON.parse(fs.readFileSync(path.resolve(dir, "../../references/subagent-multiagent-2025-2026-papers.json"), "utf8"));
const failures = [];
const slides = [...html.matchAll(/<section\s+class="[^"]*\bslide\b[^"]*"/g)];
const ids = [...html.matchAll(/<section\s+class="[^"]*\bslide\b[^"]*"[^>]*data-slide-id="([^"]+)"/g)].map((m) => m[1]);

if (slides.length < 66 || slides.length > 70) failures.push(`页面数必须为 66–70，当前 ${slides.length}`);
if (slides.length !== 68) failures.push(`目标页面数为 68，当前 ${slides.length}`);
if (ids.length !== slides.length) failures.push(`每页必须有 data-slide-id，当前 ${ids.length}/${slides.length}`);
if (new Set(ids).size !== ids.length) failures.push("data-slide-id 必须唯一");

const plain = [...js.matchAll(/"([^"\n]+)"\s*,?/g)].filter((m) => m[1].length > 40 && /这|它|用|就是|可以|不是/.test(m[1]));
if (!js.includes("plainLanguage")) failures.push("缺少大白话解释数据");
if (!js.includes("new Set(plainLanguage)")) failures.push("缺少大白话唯一性运行检查");

for (const marker of ["chapter-intro", "chapter-summary", "v1-overview", "v2-overview", "v1-to-v2"]) {
  if (!html.includes(marker)) failures.push(`缺少结构标记 ${marker}`);
}
for (const paper of catalog.papers) {
  if (!html.includes(`data-paper="${paper.id}"`)) failures.push(`课件未引用 ${paper.id}`);
}

if (failures.length) {
  console.error(`FAIL：${failures.length} 个内容结构问题`);
  failures.forEach((failure) => console.error(`- ${failure}`));
  process.exit(1);
}
console.log(`PASS：${slides.length} slides，${ids.length} unique ids，${catalog.papers.length} papers cited`);
