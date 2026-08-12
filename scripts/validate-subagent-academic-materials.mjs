import fs from "node:fs";
import path from "node:path";

export function loadPapers(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

const requiredText = [
  "id",
  "title",
  "year",
  "publication_status",
  "paper_url",
  "pdf_url",
  "research_question",
  "method",
  "experiment_scope",
  "supported_conclusion",
  "non_generalizable_boundary",
];

export function validatePapers(catalog, rootDir = process.cwd()) {
  const issues = [];
  const papers = Array.isArray(catalog?.papers) ? catalog.papers : [];
  if (catalog?.schema_version !== "1.0") issues.push("schema_version 必须为 1.0");
  if (!Array.isArray(catalog?.search_chains) || catalog.search_chains.length < 2) {
    issues.push("必须记录至少两条独立检索链");
  }
  if (papers.length < 25) issues.push(`核心论文不足 25 篇：当前 ${papers.length} 篇`);

  const ids = new Set();
  const counts = { 2025: 0, 2026: 0 };
  for (const [index, paper] of papers.entries()) {
    const label = paper?.id || `papers[${index}]`;
    for (const field of requiredText) {
      const value = paper?.[field];
      if (value === undefined || value === null || String(value).trim() === "") {
        issues.push(`${label} 缺少字段 ${field}`);
      }
    }
    if (!/^P\d{2}$/.test(paper?.id || "")) issues.push(`${label} 的 id 必须为 Pxx`);
    if (ids.has(paper?.id)) issues.push(`${label} 的 id 重复`);
    ids.add(paper?.id);
    if (![2025, 2026].includes(paper?.year)) issues.push(`${label} 年份必须为 2025 或 2026`);
    else counts[paper.year] += 1;
    if (!Array.isArray(paper?.authors) || paper.authors.length === 0) issues.push(`${label} 缺少 authors`);
    if (!Array.isArray(paper?.report_sections) || paper.report_sections.length === 0) issues.push(`${label} 缺少 report_sections 映射`);
    if (!Array.isArray(paper?.deck_slides) || paper.deck_slides.length === 0) issues.push(`${label} 缺少 deck_slides 映射`);
    if (paper?.publication_status === "formal" && (!paper?.venue || !paper?.doi)) {
      issues.push(`${label} 是正式论文但缺少 venue 或 DOI`);
    }
    if (paper?.download?.status === "downloaded") {
      if (!paper.download.local_path) issues.push(`${label} 下载成功但缺少 local_path`);
      if (!/^[a-fA-F0-9]{64}$/.test(paper.download.sha256 || "")) issues.push(`${label} 下载成功但 SHA-256 无效`);
      if (paper.download.local_path && !fs.existsSync(path.resolve(rootDir, paper.download.local_path))) {
        issues.push(`${label} 声称已下载但本地文件不存在`);
      }
    }
  }
  if (counts[2026] <= counts[2025]) {
    issues.push(`2026 年论文必须多于 2025 年：当前 ${counts[2026]} vs ${counts[2025]}`);
  }
  return { issues, counts, total: papers.length };
}

function parseArgs(argv) {
  const i = argv.indexOf("--papers");
  if (i < 0 || !argv[i + 1]) throw new Error("用法：node scripts/validate-subagent-academic-materials.mjs --papers <papers.json>");
  return argv[i + 1];
}

if (import.meta.url === new URL(`file://${process.argv[1].replaceAll("\\", "/")}`).href) {
  try {
    const filePath = parseArgs(process.argv.slice(2));
    const result = validatePapers(loadPapers(filePath), process.cwd());
    if (result.issues.length) {
      console.error(`FAIL：发现 ${result.issues.length} 个问题`);
      for (const issue of result.issues) console.error(`- ${issue}`);
      process.exitCode = 1;
    } else {
      console.log(`PASS：${result.total} 篇核心论文（2026=${result.counts[2026]}，2025=${result.counts[2025]}）`);
    }
  } catch (error) {
    console.error(`FAIL：${error.message}`);
    process.exitCode = 1;
  }
}
