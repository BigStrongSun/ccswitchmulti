#!/usr/bin/env node
const fs = require('fs/promises');
const path = require('path');
const os = require('os');
const { execFileSync } = require('child_process');
const { marked } = require('marked');

function parseArgs(argv) {
  const args = {
    width: 1440,
    height: 2400,
    maxImageHeight: 560,
    title: '',
    edge: 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  };
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (!key.startsWith('--')) continue;
    i += 1;
    const name = key.slice(2);
    if (name === 'input') args.input = value;
    else if (name === 'out-dir') args.outDir = value;
    else if (name === 'width') args.width = Number(value);
    else if (name === 'height') args.height = Number(value);
    else if (name === 'max-image-height') args.maxImageHeight = Number(value);
    else if (name === 'title') args.title = value;
    else if (name === 'edge') args.edge = value;
    else throw new Error(`Unknown argument: ${key}`);
  }
  if (!args.input || !args.outDir) {
    throw new Error('Usage: render_paged_guide.cjs --input guide.md --out-dir pages [--width 1440 --height 2400 --max-image-height 560 --title title]');
  }
  return args;
}

function fileUri(absPath) {
  return `file:///${absPath.replace(/\\/g, '/').replace(/^([A-Za-z]):/, '$1:')}`;
}

function makeSlug(text, fallback) {
  const slug = text
    .toLowerCase()
    .replace(/[^a-z0-9\u4e00-\u9fa5]+/g, '-')
    .replace(/^-|-$/g, '');
  return slug || fallback;
}

function parseMarkedPages(markdown) {
  const marker = /<!--\s*guide-page:\s*([^|]+?)\s*\|\s*(.+?)\s*-->/g;
  const matches = [...markdown.matchAll(marker)];
  if (matches.length === 0) return null;

  return matches.map((match, index) => {
    const next = matches[index + 1];
    return {
      file: match[1].trim(),
      title: match[2].trim(),
      markdown: markdown.slice(match.index + match[0].length, next ? next.index : markdown.length).trim(),
    };
  }).filter((page) => page.markdown.length > 0);
}

function parseHeadingPages(markdown) {
  const h2Matches = [...markdown.matchAll(/^##\s+(.+)$/gm)];
  const first = h2Matches[0];
  const pages = [];
  if (!first || first.index > 0) {
    pages.push({
      title: '总览',
      file: '00-overview.png',
      markdown: first ? markdown.slice(0, first.index).trim() : markdown.trim(),
    });
  }
  h2Matches.forEach((match, index) => {
    const title = match[1].trim();
    const next = h2Matches[index + 1];
    const n = pages.length;
    pages.push({
      title,
      file: `${String(n).padStart(2, '0')}-${makeSlug(title, `page-${n}`)}.png`,
      markdown: markdown.slice(match.index, next ? next.index : markdown.length).trim(),
    });
  });
  return pages.filter((page) => page.markdown.length > 0);
}

function rewriteMarkdown(section, root, guideDir) {
  return section
    .replace(/\]\(\.\.\/images\//g, `](${fileUri(path.join(root, 'docs', 'images'))}/`)
    .replace(/\]\(\.\//g, `](${fileUri(guideDir)}/`);
}

function buildCss(args) {
  return `
:root { color-scheme: light; }
* { box-sizing: border-box; }
body { margin: 0; width: ${args.width}px; min-height: ${args.height}px; background: #f6f4ee; color: #172026; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei", "Noto Sans CJK SC", sans-serif; }
.page { min-height: ${args.height}px; padding: 82px 96px 88px; background: linear-gradient(180deg, rgba(255,255,255,.84), rgba(255,255,255,.6)), #f6f4ee; }
.header-line { display: flex; justify-content: space-between; align-items: center; color: #56636b; font-size: 24px; border-bottom: 2px solid #d8d1c2; padding-bottom: 22px; margin-bottom: 42px; }
h1 { font-size: 58px; line-height: 1.14; margin: 0 0 22px; letter-spacing: 0; color: #11181d; }
h2 { font-size: 48px; line-height: 1.16; margin: 0 0 28px; letter-spacing: 0; color: #11181d; }
h3 { font-size: 32px; margin: 30px 0 12px; color: #172026; }
p, li { font-size: 27px; line-height: 1.58; }
p { margin: 16px 0; }
ul, ol { margin: 16px 0 22px 38px; padding: 0; }
li { padding-left: 8px; margin: 6px 0; }
blockquote { margin: 24px 0; padding: 18px 24px; border-left: 8px solid #397367; background: #eef5f1; color: #263a35; }
code { font-family: "Cascadia Mono", Consolas, monospace; background: #ece7da; border-radius: 6px; padding: 2px 7px; font-size: .88em; }
pre { background: #172026; color: #f7f3ea; border-radius: 8px; padding: 22px 26px; overflow: hidden; margin: 22px 0; }
pre code { background: transparent; color: inherit; padding: 0; font-size: 24px; }
img { display: block; max-width: 100%; max-height: ${args.maxImageHeight}px; object-fit: contain; margin: 28px auto; border-radius: 8px; border: 1px solid #c9c1b3; box-shadow: 0 18px 46px rgba(23, 32, 38, .16); }
table { width: 100%; border-collapse: collapse; margin: 22px 0; font-size: 24px; }
th, td { border: 1px solid #d6cebf; padding: 14px 16px; text-align: left; vertical-align: top; }
th { background: #eee7d8; }
a { color: #285f85; text-decoration: none; }
.footer { position: fixed; left: 96px; right: 96px; bottom: 40px; display:flex; justify-content:space-between; color:#6e777d; font-size:22px; }
`;
}

(async () => {
  const args = parseArgs(process.argv);
  const root = process.cwd();
  const inputPath = path.resolve(root, args.input);
  const guideDir = path.dirname(inputPath);
  const outDir = path.resolve(root, args.outDir);
  const markdown = await fs.readFile(inputPath, 'utf8');
  const pages = parseMarkedPages(markdown) || parseHeadingPages(markdown);
  if (pages.length === 0) throw new Error('No pages found');

  await fs.mkdir(outDir, { recursive: true });
  for (const entry of await fs.readdir(outDir)) {
    if (entry.endsWith('.png') || entry.endsWith('.html') || entry === 'manifest.json') {
      await fs.rm(path.join(outDir, entry), { force: true });
    }
  }

  const css = buildCss(args);
  const browserProfile = await fs.mkdtemp(path.join(os.tmpdir(), 'paged-guide-edge-'));
  const manifest = [];

  for (let i = 0; i < pages.length; i += 1) {
    const page = pages[i];
    const htmlBody = marked.parse(rewriteMarkdown(page.markdown, root, guideDir), { gfm: true, breaks: false });
    const html = `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head><body><main class="page"><div class="header-line"><span>${args.title || 'Paged Guide'}</span><span>${String(i + 1).padStart(2, '0')} / ${String(pages.length).padStart(2, '0')}</span></div>${htmlBody}<div class="footer"><span>${args.title || 'Paged Guide'}</span><span>${page.title}</span></div></main></body></html>`;
    const htmlPath = path.join(outDir, page.file.replace(/\.png$/i, '.html'));
    const pngPath = path.join(outDir, page.file);
    await fs.writeFile(htmlPath, html, 'utf8');
    execFileSync(args.edge, [
      '--headless=new',
      '--disable-gpu',
      '--hide-scrollbars',
      '--allow-file-access-from-files',
      `--user-data-dir=${path.join(browserProfile, String(i))}`,
      `--window-size=${args.width},${args.height}`,
      `--screenshot=${pngPath}`,
      fileUri(htmlPath),
    ], { stdio: 'ignore' });
    await fs.rm(htmlPath, { force: true });
    manifest.push({ title: page.title, png: page.file });
  }

  await fs.rm(browserProfile, { recursive: true, force: true });
  await fs.writeFile(path.join(outDir, 'manifest.json'), JSON.stringify({
    generatedAt: new Date().toISOString(),
    size: `${args.width}x${args.height}`,
    maxImageHeight: args.maxImageHeight,
    pages: manifest,
  }, null, 2), 'utf8');
  console.log(`Rendered ${pages.length} pages to ${outDir}`);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
