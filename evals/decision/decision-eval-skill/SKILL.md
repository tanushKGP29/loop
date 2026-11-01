---
name: decision-eval
description: Run or prepare a trustworthy evaluation of Loop's local decision-model workflows. Use when asked to benchmark Lev, compare tool-selection or task-triage decisions with a plain LLM or another System One model, label decision prompts, calibrate confidence thresholds, or prepare the decision-model evaluation report.
---

# Decision-model evaluation for Loop

Use this skill to evaluate the System One decision hook without letting the
systems under test create their own answer key. Work from the Loop fork root
that contains `evals/decision/` and `scripts/decision_eval.py`.

## Preserve the evaluation boundary

The human project owner owns correctness labels. You may draft prompt variants,
format JSONL, find duplicate prompts, and point out missing task types. Do not
assign or revise expected tool, task type, or missing-context labels from a
model response. Do not send the `labels` object to Lev, Soket, Laya, JevK5, or
any other model.

Before any model call, ask the owner to review every row in
`evals/decision/cases.to_label.jsonl`, fill its `labels` object, and save the
result as `cases.labeled.jsonl`. Keep all `holdout` rows out of threshold
selection.

## Preflight

1. Read `evals/decision/README.md` and confirm the active tool catalog matches
   the Loop session being evaluated. MCP tools need to be added to the catalog
   and represented in labelled cases before making claims about them.
2. Confirm Lev is reachable at the configured local address. Keep its service
   bound to loopback unless the owner explicitly requests network exposure.
3. Verify the required provider credentials are already configured without
   printing keys. The Soket baseline needs `SOKET_API_KEY` and a chosen model.
   A second open System One model needs its own local URL (or configured API
   key if hosted).
4. Run the ignored Lev smoke test when Lev is available:

   ```powershell
   $env:LOOP_LEV_TEST_BASE_URL = "http://127.0.0.1:8000"
   $env:LOOP_LEV_TEST_API_KEY = "local"
   cargo +stable-x86_64-pc-windows-msvc test -p loop-agent --lib live_lev_systemone_smoke_test -- --ignored --nocapture
   ```

## Run models without leaking labels

Use the provided runner. It validates all labels before it makes any request
and sends each model only a prompt, a fixed tool catalog, and fixed decision
questions.

```powershell
& ..\lev-server\.venv\Scripts\python.exe .\scripts\decision_eval.py `
  --cases .\evals\decision\cases.labeled.jsonl `
  --out-dir .\evals\decision\runs\run-001 `
  --soket-model qwen3-30b `
  --open-systemone-name laya `
  --open-systemone-base-url $env:LAYA_BASE_URL
```

Use `--skip-open-systemone` only when the second model cannot be served; record
that exception in the report. Use JevK5 rather than Laya only when the owner
has a working local JevK5 endpoint or an authorized Jev key. Never paste
secrets into a command transcript, prompt, result file, or issue log.

## Interpret results carefully

Read `report.md`, `summary.json`, and `results.jsonl` from the run directory.
Report tool accuracy, task-type accuracy, missing-context accuracy, joint
accuracy, mean/p95 latency, and reliability by confidence band. For a band
near 0.90, say both its empirical accuracy and its sample size.

Select a candidate threshold using only `tuning` rows, balancing coverage and
accuracy. Apply it once to the `holdout` rows and report that result unchanged.
The plain Soket LLM baseline exposes self-reported confidence through prompted
JSON; compare its accuracy and latency, but do not describe that confidence as
calibrated System One confidence.

If data are too sparse to justify a threshold, retain the provisional policy
and state that more labels are needed. Never hide failures, timeouts, or an
unavailable comparison model.

## Finish the phase

1. Update `docs/decision-model-spec.md` with method, model identifiers,
   environment, results, selected thresholds, and limitations.
2. Append reproducible bugs and performance findings to `notes/issues.md`.
3. Keep unit tests on `FauxDecisionModel`; do not make normal tests depend on
   Lev. Keep the live test ignored.
4. Record the three demonstration paths: high-confidence act, low-confidence
   fallback, and missing-context clarification.
5. Before opening a PR, verify the diff includes the spec, evaluation report,
   bug log, and rerun instructions.
