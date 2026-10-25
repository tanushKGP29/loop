# Loop decision-model evaluation set

This directory is the human-labelled evaluation set for Phase 6. It measures
the exact three outputs used by Loop's turn hook: tool selection, task type,
and whether critical context is missing.

`cases.to_label.jsonl` contains 40 deliberately **unlabelled** prompt cases:
30 `tuning` cases and 10 `holdout` cases. The person running the evaluation
must read every prompt and fill its `labels` object before executing a model.
Do not give labels to a model and do not use a model's response to choose one.
That keeps the benchmark independent of the systems it scores.

## Label format

Each JSONL row has this initially blank object:

```json
"labels": {
  "acceptable_tools": [],
  "task_type": null,
  "critical_context_missing": null
}
```

- `acceptable_tools` is one or more allowed tool names from
  `tool_catalog.json`. Use `["none"]` for prompts that should be answered
  without a tool. Multiple names are encouraged when the request genuinely
  has more than one acceptable first tool.
- `task_type` may use Loop's canonical values `answer`, `inspect`, `modify`,
  `execute`, or `clarify`. The runner also accepts the reviewed human labels
  `general_qa` → `answer`, `investigate` → `inspect`, `code_edit` → `modify`,
  `run_command` → `execute`, and `web_research` → `execute`. The mapping is
  applied only during scoring; no model receives labels.
- `critical_context_missing` is `true` only when Loop should ask for the
  missing information before acting. It is not a general uncertainty flag.

Copy the reviewed file to `cases.labeled.jsonl` before running an evaluation:

```powershell
Copy-Item .\evals\decision\cases.to_label.jsonl .\evals\decision\cases.labeled.jsonl
```

Keep the held-out rows (`split: "holdout"`) untouched while selecting
thresholds. The runner reports tuning and holdout metrics separately.

The catalog uses Loop's current built-in tool names. `web_fetch` and
`web_search` are only present in a live Loop registry when those optional web
tools are configured; MCP tools are likewise dynamic. Add them to both the
catalog and reviewed cases before evaluating MCP coverage. Loop currently has
no separate built-in `grep` or `glob` tool, so use `read` or `bash` only when
you personally judge either to be an acceptable first step.

## Run the evaluation

Run this from the fork root after Lev is ready. The script uses only Python's
standard library and writes raw per-case results plus a Markdown report.

```powershell
$env:SOKET_API_KEY = "..." # only if it is not already configured
python .\scripts\decision_eval.py `
  --cases .\evals\decision\cases.labeled.jsonl `
  --out-dir .\evals\decision\runs\run-001 `
  --soket-model qwen3-30b
```

If Loop is already logged in to Soket, use `--use-loop-soket-auth` instead of
exporting the key. It reads the existing `%USERPROFILE%\.loop\agent\auth.json`
credential only in memory and never writes or prints it:

```powershell
& ..\lev-server\.venv\Scripts\python.exe .\scripts\decision_eval.py `
  --cases .\evals\decision\cases.labeled.jsonl `
  --out-dir .\evals\decision\runs\soket-001 `
  --soket-model qwen3-8-27b `
  --use-loop-soket-auth `
  --skip-lev `
  --skip-open-systemone
```

The required comparison systems are:

1. Lev at `http://127.0.0.1:8000` (default).
2. A plain Soket chat-completions baseline. It must be given `--soket-model`.
3. One configured open System One endpoint, normally Laya. Set
   `LAYA_BASE_URL` and optionally `LAYA_API_KEY`; then pass
   `--open-systemone-name laya`. JevK5 can be used instead by supplying its
   local URL and `--open-systemone-name jevk5`.

The first run can omit the third model when it is not serving:

```powershell
python .\scripts\decision_eval.py `
  --cases .\evals\decision\cases.labeled.jsonl `
  --out-dir .\evals\decision\runs\lev-vs-soket `
  --soket-model qwen3-30b `
  --skip-open-systemone
```

If a provider has a transient transport or 5xx failure, retry only those rows
without overwriting successful measurements:

```powershell
& ..\lev-server\.venv\Scripts\python.exe .\scripts\decision_eval.py `
  --cases .\evals\decision\cases.labeled.jsonl `
  --out-dir .\evals\decision\runs\soket-001 `
  --skip-lev --soket-model qwen3-8-27b --use-loop-soket-auth `
  --skip-open-systemone --resume --retry-errors
```

`results.jsonl` retains every attempt for diagnosis; `summary.json` and
`report.md` use the latest attempt for each model/case.

The report includes field accuracy, joint accuracy, latency, reliability by
confidence band, and a conservative threshold table. Treat the plain-LLM
baseline's self-reported confidence as a separate, less-comparable measure;
it is not a calibrated System One probability.

## Live Rust smoke test

With Lev running, execute the ignored integration smoke test:

```powershell
$env:LOOP_LEV_TEST_BASE_URL = "http://127.0.0.1:8000"
$env:LOOP_LEV_TEST_API_KEY = "local"
cargo +stable-x86_64-pc-windows-msvc test -p loop-agent --lib live_lev_systemone_smoke_test -- --ignored --nocapture
```

The regular faux-model unit tests remain deterministic and do not require
network access.
