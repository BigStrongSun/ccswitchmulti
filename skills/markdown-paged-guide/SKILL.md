---
name: markdown-paged-guide
description: Render Markdown manuals into polished per-page PNG images and PDF deliverables. Use when Codex needs to create or update screenshot-heavy Chinese/English user guides, split long Markdown into fixed-size pages, control image scaling, or export a rendered guide folder plus PDF for product documentation.
---

# Markdown Paged Guide

Use this skill to turn a Markdown manual into fixed-size rendered pages and a PDF.

## Workflow

1. Add explicit page markers to the Markdown when page balance matters:

```markdown
<!-- guide-page: 00-overview.png | Overview -->

# Product Guide

...

<!-- guide-page: 01-setup.png | Setup -->

## Setup
```

2. Put screenshots in stable repository paths and reference them with normal Markdown image syntax.
3. Render PNG pages with `scripts/render_paged_guide.cjs`.
4. Export the PNG folder to PDF with `scripts/pngs_to_pdf.py`.
5. Inspect the most crowded pages visually before delivery.

## Rendering

Run from the repository root:

```powershell
$env:NODE_PATH='C:\Users\sunda\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\node_modules'
& 'C:\Users\sunda\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe' `
  skills\markdown-paged-guide\scripts\render_paged_guide.cjs `
  --input docs\guides\example.md `
  --out-dir docs\images\example-guide\pages `
  --width 1440 `
  --height 2400 `
  --max-image-height 560
```

The renderer uses Microsoft Edge headless. It removes old PNG/HTML/manifest files in the output directory, writes one PNG per `guide-page` marker, and writes `manifest.json`.

## PDF Export

```powershell
& 'C:\Users\sunda\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe' `
  skills\markdown-paged-guide\scripts\pngs_to_pdf.py `
  --pages-dir docs\images\example-guide\pages `
  --output output\pdf\example-guide.pdf
```

## Page Layout Rules

- Keep the first page for positioning, prerequisites, and safety notes.
- Move long checklists or process overviews to a second page when page one is crowded.
- If several pages contain two screenshots, reduce `--max-image-height` globally instead of special-casing one page.
- Use emoji sparingly for scan markers such as prerequisites, warnings, checks, and completion gates.
- Re-render PNG and PDF after every Markdown or screenshot change.
