import fs from "node:fs";

const reportPath = process.argv[2];
if (!reportPath) {
  console.error("FAIL：需要传入由真实浏览器验收导出的 JSON 报告路径");
  process.exit(1);
}
const report = JSON.parse(fs.readFileSync(reportPath, "utf8"));
const failures = [];
if (report.slides !== 68) failures.push(`浏览器页面数不是 68：${report.slides}`);
if (report.uniqueIds !== 68) failures.push(`浏览器稳定页面 ID 不是 68：${report.uniqueIds}`);
if (report.directPlain !== 68) failures.push(`主 deck 大白话不是 68：${report.directPlain}`);
if (!Array.isArray(report.viewports) || report.viewports.length !== 2) failures.push("缺少两个视口验收");
for (const item of report.viewports || []) {
  if (item.visible !== 1) failures.push(`${item.width}×${item.height} 可见页不是 1`);
  if (item.horizontalOverflow?.length) failures.push(`${item.width}×${item.height} 存在横向溢出页：${item.horizontalOverflow.join(",")}`);
  if (item.bodyOverflow) failures.push(`${item.width}×${item.height} 根页面横向溢出`);
}
if (!Array.isArray(report.themes) || report.themes.length !== 3) failures.push("缺少三主题验收");
for (const item of report.themes || []) {
  if (item.visible !== 1 || item.horizontalOverflow) failures.push(`${item.name} 主题页面可见性或溢出失败`);
  if (item.surfaceAlpha !== 1) failures.push(`${item.name} 正文表面不是不透明 alpha=1`);
}
if (!report.paperDialog?.opened || !report.paperDialog?.closedByEscape) failures.push("论文弹窗打开/Esc 关闭失败");
if (report.overviewMiniSlides !== 68) failures.push(`总览缩略页不是 68：${report.overviewMiniSlides}`);
if ((report.consoleProblems || []).length) failures.push(`浏览器控制台有问题：${report.consoleProblems.join(" | ")}`);
if (failures.length) {
  console.error(`FAIL：${failures.length} 个浏览器验收问题`);
  failures.forEach((failure) => console.error(`- ${failure}`));
  process.exit(1);
}
console.log("PASS：68页、双视口、三主题、论文弹窗、总览和控制台浏览器验收全部通过");
