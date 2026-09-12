# Engine patch — P0 `--json-events`

Wenyi Desktop drives the upstream engine (`BigDawnGhost/wenyi`) as a sidecar process and
consumes a **line-delimited JSON event stream** on stdout. That transport does not exist
upstream, so this directory carries the required patch to the engine.

Upstream is not ours to push to; the patch lives here and is applied to a local engine
checkout before building the sidecar.

## Contents

| File | Purpose |
|---|---|
| `cli-json-events.patch` | `git diff` against `trans_novel/cli.py` |
| `json_events.py` | New module → `trans_novel/json_events.py` |
| `test_json_events.py` | Tests → `tests/test_json_events.py` |
| `apply.ps1` | Applies all three to a checkout |

## Apply

```powershell
# Against a clone of the engine (default: ..\wenyi)
pwsh -File .\apply.ps1
pwsh -File .\apply.ps1 -RepoRoot D:\path\to\wenyi
```

Then verify:

```powershell
cd <engine>
uv run --no-sync pytest -q tests/test_json_events.py
uv run --no-sync python -m trans_novel --help | Select-String json-events
```

## The event contract

`--json-events` makes the engine emit one JSON object per line on **stdout**. Human-readable
output (progress summaries, errors, tracebacks) moves to **stderr** so stdout stays parseable.

| Event | Fields | Meaning |
|---|---|---|
| `stage` | `label` | Indeterminate phase started (e.g. parsing) |
| `progress` | `done`, `total`, `label` | Determinate progress for a stage |
| `usage` | `usage` | Cumulative token usage ledger |
| `done` | `outputs`, `chapters_done`, `chapters_total` (translate/prepare) or `termination`, `issues`, `suggested_changes`, `review_dir` (review) | Terminal success |
| `error` | `message` | Terminal failure; process exits non-zero |

Guarantees relied on by the desktop shell:

- stdout carries nothing but JSONL — one complete object per line, flushed immediately;
- exactly one terminal event (`done` or `error`) before exit, including on unexpected
  exceptions, so a client never blocks on a dead stream;
- events survive a closed consumer without aborting a resumable translation.

Example:

```json
{"event":"stage","label":"Parsing document…"}
{"event":"progress","done":1,"total":1,"label":"Prescanning chapter digests"}
{"event":"progress","done":0,"total":4,"label":"book"}
{"event":"error","message":"Single-paragraph fallback failed at paragraph 0"}
```

## Scope

Behavior is unchanged without the flag: the Rich terminal UI remains the default, and
`ruff check`, `ruff format --check` and the existing test suite stay green.
