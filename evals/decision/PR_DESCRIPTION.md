# PR: Add local Lev-backed decision workflows to Loop

## Summary

- Adds an opt-in `decision.provider = "systemone"` configuration and typed System One client for local Lev-compatible servers.
- Adds tool selection and task-triage workflows with conservative fallback behavior.
- Adds Faux-model unit coverage and an ignored live Lev smoke test.
- Makes decisions visible in the TUI, JSONL, session tree, and telemetry metadata.
- Adds a human-labelled 40-case evaluation harness, reusable `decision-eval` skill, results, and issue log updates.

## Safety behavior

- `provider = off` is the default and preserves pre-integration behavior.
- Server errors, malformed answers, unknown tools, and low confidence preserve the full toolset.
- `write`, `edit`, and `bash` use a stricter 0.75 advisory threshold; Loop’s normal approval gate is unchanged.
- Evaluation requests omit raw file contents, shell output, and credentials.

## Evaluation

The 40-case human-labelled comparison is in `evals/decision/RESULTS.md`.

- Lev was selected for this prototype: 70% joint accuracy on both tuning and held-out slices.
- Laya completed the third open-model run but did not meet the accuracy/reliability bar.
- Soket's plain-LLM baseline had one persistent HTTP 504 and self-reported (not calibrated) confidence.
- Lev is currently slow (about 25–31 seconds per call); add per-turn caching before broad rollout.

## Verification

```powershell
cargo +stable-x86_64-pc-windows-msvc test -p loop-agent --lib decision --jobs 1
$env:LOOP_LEV_TEST_BASE_URL = "http://127.0.0.1:8000"
$env:LOOP_LEV_TEST_API_KEY = "local"
cargo +stable-x86_64-pc-windows-msvc test -p loop-agent --lib live_lev_systemone_smoke_test -- --ignored --nocapture
```

## Follow-ups

- Verify one `decision.evaluate` span in Langfuse.
- Test a live MCP tool in the dynamic tool registry.
- Expand the independently labelled set before making production calibration claims.
- Add per-turn decision caching/hysteresis to eliminate repeated preflight latency.
