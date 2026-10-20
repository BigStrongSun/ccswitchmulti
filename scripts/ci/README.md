# CI change scope and format gates

`ci.yml` always starts for pull requests and pushes to `main`. `Change Scope`
uses the complete local Git diff, without GitHub API file-list limits:

- Pull requests compare the head with the merge base of the event's base/head SHAs.
- Pushes compare the event's `before` and `after` SHAs, covering every pushed commit.
- Only root Markdown files/`LICENSE`, and `.md`, `.rst`, `.txt`, `.png`, `.jpg`,
  `.jpeg`, `.gif`, `.webp` files under `docs/`, qualify as documentation.
- Files must have regular, nonexecutable modes on both sides. Scripts, source,
  MDX, SVG, symlinks, submodules, configuration, and unknown paths run full checks.
- Rename detection is disabled so both removed and added paths are inspected.
  Moving code into a Markdown file still runs full checks.
- Empty diffs, missing/zero/unavailable revisions, unsupported events, malformed
  payloads or diffs, and Git failures conservatively run full checks.

A successful classification with an explicit `docs` result skips the single
`Frontend Checks` and `Rust Formatting` jobs. The backend matrix always expands
with these exact check names, independently of the runner it uses:

- `Backend Checks (ubuntu-22.04)`
- `Backend Checks (windows-latest)`
- `Backend Checks (macos-latest)`

For docs-only changes, all three run only a short result gate on `ubuntu-latest`;
they do not check out the repository, set up Rust, restore caches, install system
dependencies, or compile. This retains three small Ubuntu job costs while avoiding
Windows/macOS runners. GitHub evaluates [job conditions before matrix expansion][job-if],
so skipping the entire matrix would not produce its individual required names
([runner issue #952][matrix-skip]). There are no workflow path filters:
[skipped workflows may leave required checks pending][syntax]. The single
frontend check can use [GitHub's successful skipped-job status][conditions].

For full checks, the frontend installs dependencies once, checks formatting first,
then runs the original type check and unit tests. Rustfmt runs once in parallel on
Ubuntu, without installing Tauri system dependencies or compiling. The three
backend jobs wait for both jobs and retain Clippy (`-D warnings`) and Cargo tests
on Ubuntu, Windows, and macOS. Failed prerequisites use Ubuntu result gates
instead of allocating the three build platforms. The first step requires successful
classification; docs-only changes pass immediately, while all other scope values
require successful frontend and Rustfmt results. Each heavy step also requires a
non-docs scope and the default success condition. Thus neither docs-only changes
nor failed gates can start dependencies or compilation. Unknown/empty scope output
takes the full-check path.

Waiting for the complete frontend job reuses its one dependency installation and
avoids backend compute on frontend test failures, at the cost of serializing
successful backend runs behind frontend type checking and tests. A cancelled run
is still cancelled; the existing concurrency policy is unchanged.

Run the classifier regression tests with Python's standard library:

```sh
python3 -B -m unittest discover -s scripts/ci -p 'test_*.py'
```

Tests create disposable Git repositories and cover real edits, rename/delete,
executable modes, divergent PR bases, multi-commit pushes, malformed inputs,
and the fail-closed CLI output. No frontend or Rust dependencies are needed.

[conditions]: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-jobs-with-conditions
[syntax]: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpull_requestpull_request_targetpathspaths-ignore

[job-if]: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idif
[matrix-skip]: https://github.com/actions/runner/issues/952
