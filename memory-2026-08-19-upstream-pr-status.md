# CC Switch upstream PR status audit — 2026-08-19

## Scope and sources

- Official repository: `farion1231/cc-switch`.
- Local checkout remotes were checked; `origin/main` was fetched at `0b5da510` (release notes for v3.20.0, 2026-08-18).
- GitHub connector search found 11 PRs authored by `BigStrongSun`.
- Official GitHub PR pages/API were used for state and merge metadata. Codex web search independently found the official PR index and issue #5989; matrix-websearch was invoked independently but returned no reliable PR result for this repository.

## Verified status

| PR | Topic | Status on 2026-08-19 | Adopted? |
| --- | --- | --- | --- |
| #4067 | router workspace + external OpenAI API | closed, not merged | no |
| #4187 | custom model catalog projection | open, draft | no |
| #4192 | preserve user config on route switch | open, draft | no |
| #4194 | strip provider-owned common-config fields | open, draft | no exact adoption |
| #4292 | active SQLite history visibility repair | open, ready | no |
| #4294 | Session Manager history repair UI | open, draft | no |
| #4381 | preserve live settings during backup restore | open, ready | no |
| #4727 | strip Responses-Lite header | closed by maintainer | explicitly declined; do not count as pending |
| #5990 | whole CCSwitchMulti upstream proposal | open, draft | no; discussion only |
| #6159 | Responses Lite `additional_tools` conversion | open, ready, awaiting code-owner review | no |
| #6530 | coalesce commentary with pending tool calls | open, ready, awaiting code-owner review | no |

## Same-class upstream work

The official `origin/main` contains related fixes, but not the exact BigStrongSun PRs above: model-catalog and provider/common-config work (`413c09e0`, `473c2aaa`, `93f56198`, `40cac1a6`), history/session work (`69341db2`, `e606adfa`), and proxy conversion hardening (`27ce0a51`, `9ca1a41`). These should be described as related or overlapping fixes, not as adoption of our PRs.

The exact commits for #6159 (`31d8a937`) and #6530 (`246a475f`) are not ancestors of official `origin/main`; both PRs remain open.

## Outreach

On 2026-08-19 a single bilingual, focused-maintainer comment was posted to PR #5990, asking the maintainer to review #6159/#6530 first and then the smaller focused PRs. No comment was posted to the already-closed #4727.

## New protocol-conversion PR

- PR #6615 was created accidentally against `BigStrongSun/ccswitchmulti`, which is outside the official fork network; GitHub showed 0 commits and 0 files. It was closed immediately.
- The same commit was pushed to `BigStrongSun/ccswitchmulti-fork-archive` and correctly submitted as [PR #6616](https://github.com/farion1231/cc-switch/pull/6616).
- #6616 changes only `transform_codex_chat.rs`: unsupported or malformed Responses tool entries are recorded and rejected with a deterministic `TransformError` instead of being silently dropped. Full `transform_codex_chat` tests pass: 90/90.
- The PR body is bilingual and explains scope, root cause, observable effect, and validation. A maintainer comment was added because reviewer-request API returned 403.
