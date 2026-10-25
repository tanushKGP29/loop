# Phase 6 Decision-Model Evaluation Results

**Date:** 2026-10-25
**Decision:** retain **Lev** as Loop's decision provider for this prototype. Laya is a valid System One comparison endpoint, but did not meet the accuracy or reliability bar on this set.

## Method

- 40 prompts were labelled by the project owner before any model request: 30 tuning cases and 10 held-out cases.
- Each case scores the same independent decisions used by Loop: first tool, task type, and critical-context-missing.
- Tool scoring accepts any human-approved first tool, rather than forcing one arbitrary implementation path.
- The models received only the prompt, fixed seven-tool catalog, and decision questions. Labels never left the evaluator.

## Results

| Model | Split | Tool | Task type | Missing context | Joint | Mean latency | p95 latency |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Lev (`interfaze-ai/lev`) | tuning (30) | 83.3% | 90.0% | 90.0% | 70.0% | 26.80 s | 31.42 s |
| Lev | held-out (10) | 90.0% | 80.0% | 90.0% | 70.0% | 25.15 s | 27.03 s |
| Soket plain LLM (`qwen3-8-27b`) | tuning (29 successful) | 93.1% | 69.0% | 89.7% | 58.6% | 18.54 s | 49.81 s |
| Soket plain LLM | held-out (10) | 90.0% | 60.0% | 100.0% | 60.0% | 23.83 s | 67.01 s |
| Laya 0.4.15 | tuning (30) | 40.0% | 63.3% | 70.0% | 6.7% | 1.28 s | 1.63 s |
| Laya 0.4.15 | held-out (10) | 50.0% | 30.0% | 60.0% | 0.0% | 1.49 s | 2.09 s |

The Soket tuning denominator is 29 because `t08_missing_target_file` returned HTTP 504 twice. The baseline's confidence was produced by prompted JSON and is **not** treated as calibrated.

Laya completed all 40 calls through `/v1/systemone`. Its installed runtime reported a CPU fallback despite `LAYA_DEVICE=cuda`, plus an upstream warning that its checkpoint temperature values were repaired at load time. Its speed is useful evidence, but its accuracy and sparse, inconsistent high-confidence results make it unsuitable for Loop's gating policy on this data.

## Lev reliability and policy selection

Lev tool reliability on the **tuning** split was:

| Confidence band | Cases | Mean confidence | Correct |
| --- | ---: | ---: | ---: |
| [0.00, 0.50) | 13 | 29.2% | 69.2% |
| [0.50, 0.70) | 5 | 55.2% | 80.0% |
| [0.70, 0.90) | 7 | 82.7% | 100.0% |
| [0.90, 1.00] | 5 | 93.5% | 100.0% |

Selected prototype thresholds:

- **General tool action:** confidence `>= 0.50`.
- **Destructive tool action (`write`, `edit`, `bash`):** confidence `>= 0.75`; otherwise retain the full toolset / existing approval flow.
- **Critical-context clarification:** retain the existing conservative Noul probability `>= 0.75` plus derived confidence `>= 0.50` gate.

The general choice is supported by 16/17 correct promoted tuning cases (94.1%) and 4/4 correct promoted held-out cases at 0.50. At 0.75, Lev was 11/11 on tuning promotions and 3/3 on held-out promotions. The destructive and Noul thresholds deliberately remain conservative: this 40-case set does not contain enough destructive-only or high-confidence Noul examples to claim calibrated safety. Loop's existing tool approval gate remains authoritative.

## Operational decision

Lev is the selected provider because it has the best task-triage and joint accuracy, and its System One confidence produced a useful fallback boundary. However, its 25–31 second per-call latency is a confirmed usability problem. Before broad use, add per-turn decision caching or avoid reevaluating unchanged user context/tool registries. Keep the TUI decision line/spinner visible during evaluation.

## Reproducibility and artifacts

- Labels: `cases.labeled.jsonl`
- Fixed catalog: `tool_catalog.json`
- Lev run: `runs/lev-001/`
- Plain-Soket run: `runs/soket-001/`
- Laya run: `runs/laya-001/`
- Evaluator: `../../scripts/decision_eval.py`
- Reusable Loop skill: `decision-eval-skill/SKILL.md`

The evaluator is resumable and uses latest-attempt scoring. It keeps raw attempts in each run's `results.jsonl` and writes a `summary.json` and `report.md` beside it.

## Remaining non-blocking checks

- Inspect one `decision.evaluate` child span in Langfuse.
- Test a real enabled MCP tool so the dynamic-registry prompt includes it.
- Repeat with a larger, independently labelled set before treating the confidence bands as production calibration.
