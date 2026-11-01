# Lev Decision Model Interface

## Purpose

Loop can optionally consult a local [Lev](https://github.com/Abhinavexists/lev)
System One server for two workflows: tool selection and task triage. Lev
replaces Jev in this project. It is advisory only: Loop's primary model still
decides whether to make a tool call, and Loop's existing tool-approval gate
remains authoritative.

With `decision.provider = "off"` (the default), or on any decision-model
error, Loop builds exactly the same full tool list as before this feature.

## Configuration

The global `%USERPROFILE%\\.loop\\agent\\settings.json` accepts:

```json
"decision": {
  "provider": "systemone",
  "base_url": "http://127.0.0.1:8000",
  "api_key": "local",
  "timeout_s": 60
}
```

`DecisionSettings` defaults to `provider: off`, the local loopback URL,
`api_key: local`, and a 60-second normal timeout. The first request has a
minimum 120-second allowance for Lev model loading and kernel compilation.
The client makes one retry for a transport failure or HTTP 5xx response.

## Protocol and Domain Types

`SystemOneHttpModel` sends `POST {base_url}/v1/systemone` with the typed
`DecisionRequest` body. Its fields are `base_url`, `api_key`, optional `model`,
normal timeout, and first-call timeout. Errors are typed as `Unavailable`,
`Rejected`, `InvalidResponse`, or `Exhausted` (for the offline fake).

```rust
#[async_trait::async_trait]
pub trait DecisionModel: Send + Sync {
    async fn decide(&self, req: DecisionRequest) -> Result<Decision, DecisionError>;
}
```

`DecisionRequest` contains JSON `state` and a `BTreeMap` of question IDs to:

- `Choice { instructions, criteria: BTreeMap<name, Option<description>> }`.
  `None` is serialized as the protocol-required JSON `null`.
- `Score { instructions, criteria: Vec<Option<description>> }`, validated to
  contain 2–10 ordered levels.
- `Noul { instructions }`, a binary yes/no question.

All answers are normalized into one `DecisionAnswer` enum. Choice and Score
retain their protocol confidence. Noul retains `probability_yes` and derives
one confidence value as `|2p - 1|`, because the protocol has no separate Noul
confidence field. Responses are rejected if they omit a requested answer, use
the wrong answer shape, choose an unknown option/level, or contain invalid
probabilities or confidence.

`FauxDecisionModel` is a FIFO scripted implementation for deterministic
offline tests. There is no pre-existing `LlmDecisionModel` in this Loop fork;
the native Loop model path is retained as the fallback instead of adding an
unrelated live provider.

## Policy and Turn-Loop Hook

`DecisionPolicy` maps normalized confidence to `Act`, `Confirm`, or
`Fallback`:

| Condition | Result |
| --- | --- |
| Missing, non-finite, or below `0.50` confidence | `Fallback` |
| Non-destructive confidence at least `0.50` | `Act` |
| Destructive confidence `0.50..0.75` | `Confirm` |
| Destructive confidence at least `0.75` | `Act` |

`DecisionToolSelector` is installed only when `provider = systemone`. In the
hand-written hook in `stream_assistant_response`, it sends the latest user
message plus tool names/descriptions to Lev in one batched request containing:

- `tool`: a Choice over the live tool registry plus `none`;
- `task_type`: an independent Choice over `answer`, `inspect`, `modify`,
  `execute`, and `clarify`;
- `critical_context_missing`: an independent Noul asking whether execution
  should wait for clarification.

The request answers are independent by design. A high-confidence known tool
produces a one-tool advisory menu; `none`, `Confirm`, `Fallback`, an
unavailable server, or an invalid response preserves the complete original
tool list. If the missing-context Noul meets the conservative provisional
threshold, Loop returns a clarification request before exposing tools or
calling the main model. No shell output, file content, or credentials are sent
in this request.

The built-in `write`, `edit`, and `bash` tools are classified as destructive
for this advisory decision: below `0.75` they are not promoted into a narrowed
menu, while Loop's existing approval gate still controls actual execution.
The `0.50`/`0.75` thresholds are provisional conservative defaults. Phase 6
must calibrate final thresholds against project-specific evaluation data before
the triage thresholds are treated as final.

## Task-type Routing

When clarification is not required, `task_type` selects a resource named
exactly `answer`, `inspect`, `modify`, `execute`, or `clarify`. A matching skill
is preferred; otherwise a matching prompt template is selected. The selected
body is added to that turn's system prompt in a labelled block. No matching
resource means normal Loop prompting continues unchanged. Template bodies are
used without command arguments, so routing cannot accidentally substitute text
from an unrelated slash-command invocation.

The existing `before_tool_call` approval enforcement is unchanged. The
decision model never changes tool arguments and cannot grant permission.

## Verification

Run from the fork root in the Visual Studio x64 environment:

```powershell
cargo +stable-x86_64-pc-windows-msvc test -p loop-agent --lib decision --jobs 1
cargo +stable-x86_64-pc-windows-msvc test -p loop-app-core --lib decision_settings_parse_the_lev_example --jobs 1
cargo +stable-x86_64-pc-windows-msvc check -p loop-app-core --jobs 1
```

The focused test suite covers Choice, Score, and Noul decoding; the Noul
confidence calculation; score-level validation; conservative policy thresholds;
both batched workflow questions; FIFO/exhaustion behavior for the offline fake;
and the one-retry HTTP path.
The app-core test verifies the exact snake_case Lev configuration block and the
safe default of `provider = off`.

## Decision Visibility

Every successfully validated and applied System One decision emits a structured
`AgentEvent::Decision`, without recording the raw request state. The interactive
TUI receives it as an info notice such as:

```text
◆ decision: tool=read conf=0.93 (212ms) → narrowed_toolset
```

The event contains the validated answers, selected tool and confidence, action
taken, model identifier, latency, and timestamp. When `provider = systemone`,
the harness appends one JSON object per event to
`%USERPROFILE%\\.loop\\agent\\decisions.jsonl`. The log is append-only and
does not contain the user task state, file contents, or credentials.

The event is also stored at its point in the session tree as custom entry type
`decision`. `/tree` now displays the most recent ten persisted decision entries
before its existing branch-navigation notice.

When telemetry is enabled, `decision.evaluate` is a child span on the current
Langfuse trace. Its structured output is the answer map; its metadata includes
`tool`, `tool_confidence`, `action`, `latency_ms`, and `model`. This uses
Loop's existing custom `obs::span` and `ObservationExt::record_metadata`
support rather than a separate Langfuse client.

## Phase 6 Evaluation Status

The Phase 6 set is human-labelled and lives in `evals/decision/`. It contains
40 cases: 30 tuning cases and 10 held-out cases. Every case scores the same
three decisions as Loop's hook: acceptable first tool, task type, and whether
critical context is missing. The runner sends models only the prompt, the
fixed tool catalog, and the questions; labels are validated locally before any
request and are never transmitted.

Human labels may use more descriptive task categories. The evaluator maps them
to Loop's real `TaskType` enum only while scoring:

| Human label | Loop task type |
| --- | --- |
| `general_qa` | `answer` |
| `investigate` | `inspect` |
| `code_edit` | `modify` |
| `run_command` | `execute` |
| `web_research` | `execute` |

The first Lev run (`evals/decision/runs/lev-001/`) completed all 40 cases with
no transport or protocol failures. These are preliminary single-run results;
the set is too small to make a strong calibration claim, especially in the
high-confidence bands.

| Split | Tool | Task type | Missing context | Joint | Mean latency | p95 latency |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Tuning (n=30) | 83.3% | 90.0% | 90.0% | 70.0% | 26.80 s | 31.42 s |
| Holdout (n=10) | 90.0% | 80.0% | 90.0% | 70.0% | 25.15 s | 27.03 s |

For the tuning set, a tool confidence threshold of 0.50 covered 17/30 cases
and was correct on 16/17 (94.1%). The unchanged holdout covered 4/10 cases at
that threshold and was correct on 4/4. Above 0.60, tuning accuracy was 13/13,
but coverage fell to 13/30. The 0.90+ confidence band contains only 5 tuning
and 2 held-out cases, despite being correct on all of them, so it is not enough
to claim reliable 0.90 calibration.

Keep the existing 0.50 general confidence floor and 0.75 destructive-tool
floor provisionally. Choose final thresholds only after the plain-Soket
baseline and second open decision-model run have been recorded. The current
local Lev latency is a deployment concern: this evaluation observed roughly
25–31 seconds per decision, which is far too slow to call once per tool-loop
iteration without caching or another latency mitigation.

## Phase 6 Final Comparison and Threshold Decision

All three required systems have now run against the same 40 human-labelled
cases (30 tuning, 10 held-out). The complete reproducible report is
`evals/decision/RESULTS.md`; raw attempts and per-model reports remain under
`evals/decision/runs/`.

| Model | Tuning joint | Held-out joint | Mean latency (tuning / held-out) |
| --- | ---: | ---: | ---: |
| Lev | 70.0% | 70.0% | 26.80 s / 25.15 s |
| Plain Soket `qwen3-8-27b` | 58.6% (n=29) | 60.0% | 18.54 s / 23.83 s |
| Laya 0.4.15 | 6.7% | 0.0% | 1.28 s / 1.49 s |

Lev remains the configured decision provider. At the 0.50 tool threshold, it
was correct on 16/17 tuning promotions and 4/4 held-out promotions. At 0.75,
it was correct on 11/11 tuning and 3/3 held-out promotions. The selected
prototype policy is therefore the existing 0.50 general floor and 0.75
stricter floor for destructive tools. The missing-context Noul gate stays at
probability 0.75 plus derived confidence 0.50. This is a conservative product
threshold decision, not a claim of statistically established calibration:
high-confidence and destructive-only samples remain small.

Laya was served successfully through the same System One protocol but was not
selected: its tool and joint accuracy were materially lower, and its installed
runtime fell back to CPU and warned about repaired checkpoint temperatures.
The plain Soket baseline had one persistent 504 and only self-reported
confidence, which is not comparable to System One probabilities.

The remaining engineering priority is per-turn caching or hysteresis. Repeating
a Lev decision for unchanged context costs roughly 25-31 seconds per call.

## Per-turn Decision Cache

The hand-written turn loop now keeps one successful `TurnDecisionCache` for
the current agent-loop run. The cache key is the complete normalized System One
request, which includes the latest user message plus the live tool names and
descriptions. A repeated tool-loop iteration with the same request reuses the
validated advisory result and does not make another Lev request or emit a
duplicate `decision.evaluate` span. A steering/user message or any tool
registry update changes that request and forces a new assessment.

Failed, malformed, or unavailable decisions are never cached. They retain the
existing safe full-toolset fallback. The focused unit suite verifies reuse and
tool-registry invalidation with `FauxDecisionModel`; the ignored live Lev smoke
test still passes. An interactive timing run should confirm the expected removal
of repeated 25-31 second preflight waits within one turn.
