import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

const base = "43eaf07355af145aebfee301801779e824d4c221";
const release = "3217f725";
const tip = "741e802f1da287c182e81b479bd12ef6fa8c1fd9";
const output = resolve(
  "docs/audits/2026-09-06-v3.20.1-upstream-commit-matrix.md",
);

const git = (...args) =>
  execFileSync("git", args, {
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  }).trim();

const isAncestor = (ancestor, descendant) => {
  try {
    execFileSync("git", ["merge-base", "--is-ancestor", ancestor, descendant]);
    return true;
  } catch {
    return false;
  }
};

const records = git(
  "log",
  "--reverse",
  "--no-merges",
  "--date=short",
  "--format=%H%x1f%ad%x1f%s",
  `${base}..${tip}`,
)
  .split(/\r?\n/)
  .filter(Boolean)
  .map((line) => {
    const [hash, date, subject] = line.split("\x1f");
    const files = git("diff-tree", "--no-commit-id", "--name-only", "-r", hash)
      .split(/\r?\n/)
      .filter(Boolean);
    return { hash, date, subject, files };
  });

if (records.length !== 131) {
  throw new Error(`Expected 131 upstream commits, found ${records.length}`);
}

const explicit = new Map([
  [
    "413c09e0",
    [
      "already-covered",
      "Current CCSwitchMulti separates user-owned model_catalog_json from CCSM-owned projection and has ownership/consistency regressions.",
    ],
  ],
  [
    "40cac1a6",
    [
      "already-covered",
      "Current capability catalogue and provider UI already preserve per-model reasoning levels with dynamic official metadata precedence.",
    ],
  ],
  [
    "273c9cc2",
    [
      "already-covered",
      "Current model capability registry already marks exact glm-5.3 variants text-only while leaving glm-5.3v image-capable, with normalization regressions.",
    ],
  ],
  [
    "7dc0a725",
    [
      "adopted",
      "Task 3 semantically migrated Grok 4.5/4.6 prices and the DeepSeek V4 Flash 0731 alias with guarded price repair tests.",
    ],
  ],
  [
    "bad9c151",
    [
      "adopted",
      "Task 3 migrated current DeepSeek V4 peak-tier rows and Gemini 3.7 Flash introductory pricing with seed and historical-repair tests.",
    ],
  ],
  [
    "460aa8c7",
    [
      "adopted",
      "Task 3 migrated Fable/Mythos 5.1 rows and the exact-guard Sonnet 5 standard-price repair without overwriting user prices.",
    ],
  ],
  [
    "741e802f",
    [
      "adopted",
      "Task 3 added the GLM-5.3 seed without a schema bump or repair overwrite and proved user price preservation.",
    ],
  ],
  [
    "c39c9032",
    [
      "adopted",
      "Task 4 adds the WSL ERROR_NOT_SUPPORTED rename fallback to CCSwitchMulti's stronger Windows atomic-write recovery path, with a focused classifier regression.",
    ],
  ],
  [
    "3c592d93",
    [
      "already-covered",
      "The current CCSwitchMulti WiX template already escapes Handlebars-adjacent registry-key separators as double backslashes.",
    ],
  ],
  [
    "967daa1a",
    [
      "adopted",
      "Task 4 checks the Skill SSOT directory before trusting a cached hash, so database-only restores expose missing files as an available repair.",
    ],
  ],
  [
    "de9af49a",
    [
      "adopted",
      "Task 4 reconstructs Windows CLI detection PATH from process, HKCU, and HKLM state; prioritizes the effective PATH target; and adds Codex and Claude standalone installer locations.",
    ],
  ],
  [
    "d4fefefc",
    [
      "adopted",
      "Task 4 applies the saved theme before first paint and delays the first Windows show until the non-about page load completes, while preserving silent startup.",
    ],
  ],
  [
    "4549d290",
    [
      "adopted",
      "Task 4 grants process:allow-exit because the database recovery UI invokes the Tauri process exit API directly.",
    ],
  ],
  [
    "dfb2e523",
    [
      "adopted",
      "Task 4 commits c225a1b0/1b00aeb5 preserve SQL values and sequences, validate staged restores, and atomically publish safety snapshots under a backup-file lock.",
    ],
  ],
  [
    "c9fe340b",
    [
      "adopted",
      "Task 4 commit 0e3fe111 unifies WebDAV/S3/manual restore locking, coordinates Skill DB/SSOT state, and rebuilds every live projection after import.",
    ],
  ],
  [
    "c911c7e3",
    [
      "adopted",
      "Task 4 commit f85984ba preserves unmanaged local Prompt content when restore enables no managed Prompt, with a focused projection regression.",
    ],
  ],
  [
    "5ca9459d",
    [
      "deferred",
      "Task 5 confirmed CCSwitchMulti has no Pi session parser or dedup ledger yet; defer the Pi-specific lookup index to Task 8 so no orphan schema is created.",
    ],
  ],
  [
    "092ea1f3",
    [
      "deferred",
      "Task 5 confirmed the auto/manual scan toggle is settings and frontend behavior rather than a database migration; evaluate it with the Task 9 usage UI batch.",
    ],
  ],
  [
    "bcee61be",
    [
      "adopted",
      "Task 5 commit db4c6fe9 maps Claude incremental byte cursors onto CCSwitchMulti schema v21, preserving legacy line cursors and other parsers.",
    ],
  ],
  [
    "f8d97348",
    [
      "adopted",
      "Task 5 commit db4c6fe9 fingerprints the committed Claude prefix, pins truncation or rewrites at EOF, keeps incomplete tails resumable, and makes cursor advancement transactional.",
    ],
  ],
  [
    "f05e2033",
    [
      "adopted",
      "Task 5 commit db4c6fe9 surfaces permanent rewrite skips in session sync errors instead of reporting a silent success.",
    ],
  ],
  [
    "5a040348",
    [
      "already-covered",
      "CCSwitchMulti commit 255a6771 and the bundled DeepSeek catalogue already set supports_search_tool=false with MCP visibility regressions.",
    ],
  ],
  [
    "db346128",
    [
      "already-covered",
      "The completed GPT-6/Astra metadata integration on main includes the OAuth client identity correction and runtime canary evidence.",
    ],
  ],
  [
    "bc4ed66d",
    [
      "already-covered",
      "The current DeepSeek catalogue and MultiRouter capability projection already carry supports_parallel_tool_calls with compiler tests.",
    ],
  ],
]);

const notApplicable = new Set([
  "ceef0a52",
  "36ed280d",
  "c98cc3a9",
  "bef46cd5",
  "a7f073e9",
  "0cd922c5",
  "af31a87b",
  "18ca2da0",
  "0b5da510",
  "0ae561b8",
  "58687bd6",
  "9485cf2f",
  "3217f725",
  "527b56f8",
  "d05a11cc",
]);

function taskFor({ subject, files }) {
  const text = `${subject} ${files.join(" ")}`.toLowerCase();
  const hasProxy = files.some((file) =>
    file.startsWith("src-tauri/src/proxy/"),
  );
  const hasFrontend = files.some(
    (file) =>
      file.startsWith("src/components/") ||
      file.startsWith("src/hooks/") ||
      file === "src/App.tsx",
  );
  const hasPresetData = files.some(
    (file) =>
      file.startsWith("src/config/") ||
      file.includes("preset") ||
      file.startsWith("assets/partners/"),
  );
  if (/feat\(pi\): add native coding agent/.test(text)) return 8;
  if (
    /pi session|session dedup|byte-cursor|session scan|pinned-rewrite/.test(
      text,
    )
  )
    return 5;
  if (
    /oauth|follow-login|managed account|auth writes|auth cleanup|bearer token|workspace account|credential-aware|official-auth|requires_openai_auth/.test(
      text,
    )
  )
    return 6;
  if (
    hasProxy ||
    /websearch|alpha search|system message|xai native|moonshot|reasoning_content|agent_message|whole-float|rollout|namespace sse/.test(
      subject.toLowerCase(),
    )
  )
    return 7;
  if (
    /pricing|catalog|capabilit|contextwindow|reasoning level/.test(text) &&
    !/provider form|preset/.test(text)
  )
    return 3;
  if (
    /wsl|backup|sync|restore|detect|startup|fouc|wix|env-check|skills|prompt files|terminal|process:allow-exit/.test(
      text,
    )
  )
    return 4;
  if (hasFrontend || hasPresetData || /preset|provider form|ui|a11y/.test(text))
    return 9;
  return 9;
}

function classify(record) {
  const short = record.hash.slice(0, 8);
  if (explicit.has(short)) return explicit.get(short);
  if (notApplicable.has(short)) {
    return [
      "not-applicable",
      "Upstream release/docs/CI/refactor-only scope is not copied into the CCSwitchMulti product migration; equivalent fork workflow hygiene is reviewed separately.",
    ];
  }
  const task = taskFor(record);
  return [
    "rewritten",
    `Task ${task} semantic review: preserve the upstream intent but reject direct branch/cherry-pick integration until it is mapped to current CCSwitchMulti ownership and tests.`,
  ];
}

function subsystem(files) {
  const groups = [];
  const any = (predicate) => files.some(predicate);
  if (
    any(
      (file) =>
        file.includes("src-tauri/src/database") ||
        file.includes("session_usage"),
    )
  )
    groups.push("database/usage");
  if (any((file) => /codex_oauth|auth\.rs|oauth/.test(file)))
    groups.push("OAuth/auth");
  if (any((file) => file.startsWith("src-tauri/src/proxy/")))
    groups.push("proxy/protocol");
  if (any((file) => /^src-tauri\/src\/(services|commands)\//.test(file)))
    groups.push("backend services");
  if (
    any(
      (file) =>
        file.startsWith("src/components/") ||
        file.startsWith("src/hooks/") ||
        file === "src/App.tsx",
    )
  )
    groups.push("frontend");
  if (
    any(
      (file) =>
        file.startsWith("src/config/") ||
        file.includes("preset") ||
        file.startsWith("assets/partners/"),
    )
  )
    groups.push("presets/assets");
  if (any((file) => file.startsWith(".github/"))) groups.push("CI");
  if (any((file) => file.startsWith("docs/") || /README|CHANGELOG/.test(file)))
    groups.push("docs");
  if (any((file) => /package\.json|Cargo\.|tauri\.conf|wix\//.test(file)))
    groups.push("build/package");
  return groups.length ? groups.join(", ") : "other";
}

const escapeCell = (value) =>
  String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
const counts = new Map();
const rows = records.map((record, index) => {
  const [disposition, reason] = classify(record);
  counts.set(disposition, (counts.get(disposition) ?? 0) + 1);
  const boundary = isAncestor(record.hash, release) ? "v3.20.1" : "post-tag";
  return `| ${index + 1} | \`${record.hash.slice(0, 8)}\` | ${record.date} | ${boundary} | ${escapeCell(record.subject)} | ${subsystem(record.files)} | ${disposition} | ${escapeCell(reason)} |`;
});

const releaseCount = records.filter((record) =>
  isAncestor(record.hash, release),
).length;

const postTagCount = records.length - releaseCount;
const summary = [...counts.entries()]
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([name, count]) => `- ${name}: ${count}`)
  .join("\n");

const markdown = `# CC Switch v3.20.1 → CCSwitchMulti v3.20.1-1 commit matrix

## Frozen audit boundary

- CCSwitchMulti base: \`${git("rev-parse", "HEAD")}\`
- Upstream merge-base: \`${base}\`
- Upstream release: \`v3.20.1@${git("rev-parse", "v3.20.1^{}")}\`
- Upstream audit tip: \`${tip}\`
- Upstream-only non-merge commits: ${records.length} (${releaseCount} through v3.20.1, ${postTagCount} post-tag)
- Patch-equivalent commits reported by \`git cherry main origin/main\`: 0; semantic coverage is therefore documented explicitly rather than inferred from patch IDs.

The disposition in this first-pass matrix records the integration decision, not a claim that later implementation work is already complete. \`rewritten\` means direct cherry-pick/merge is rejected and the upstream intent is assigned to a specific implementation task. Tasks 3–10 update rows with landed CCSwitchMulti commit/test evidence; unsupported candidates may be changed to \`deferred\` with a concrete reason.

## Counts

${summary}

## Commit disposition

| # | Commit | Date | Boundary | Subject | Touched subsystem | Initial disposition | Evidence / owner |
| -: | --- | --- | --- | --- | --- | --- | --- |
${rows.join("\n")}

## Completeness check

- Expected commits: 131
- Generated unique commits: ${new Set(records.map((record) => record.hash)).size}
- Duplicate commits: ${records.length - new Set(records.map((record) => record.hash)).size}
- Missing commits: ${131 - records.length}
- Frozen tip verified: ${git("rev-parse", tip) === tip ? "yes" : "no"}

## Review boundaries

- No row authorizes a whole-branch merge. Every \`rewritten\` row must pass the current CCSwitchMulti invariant and focused RED/GREEN gate in its owning task.
- Official release/version commits and upstream-only CI topology are \`not-applicable\`; CCSwitchMulti maintains its own derived version, release notes, signing, updater, and three-platform workflow.
- The five \`already-covered\` rows have current-tree source/test evidence; later checkpoints still re-evaluate them if a dependency changes.
- Any upstream commit after \`${tip.slice(0, 8)}\` is a late arrival and cannot silently enter the frozen 3.20.1-1 scope.
`;

writeFileSync(output, markdown, { encoding: "utf8" });
console.log(`Wrote ${output}`);
console.log(
  `commits=${records.length} release=${releaseCount} postTag=${postTagCount}`,
);
console.log(summary);
