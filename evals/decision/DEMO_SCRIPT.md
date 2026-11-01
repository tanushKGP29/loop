# 60-second Phase 6 Demo Script

Use the development binary with Lev healthy at `http://127.0.0.1:8000`. Keep the TUI and, if desired, the Langfuse trace view visible. Do not show API keys or `auth.json`.

1. **High-confidence action (about 20 seconds).** Ask: `Inspect crates/loop-agent/src/decision.rs. Do not modify files. Explain how a DecisionRequest is validated.` Show the `◆ decision: tool=read ... → narrowed_toolset` line and the answer.
2. **Low-confidence fallback (about 20 seconds).** Ask a deliberately underspecified request, for example: `Improve the parser.` Show the decision line indicating fallback/full toolset (or the normal full-tool behavior) rather than forced narrowing.
3. **Missing-context clarification (about 20 seconds).** Ask: `Rename the parser function.` Show Loop asking what symbol/file should be changed before taking action.

After recording, show `/tree` and the last three JSONL records:

```powershell
Get-Content "$env:USERPROFILE\.loop\agent\decisions.jsonl" -Tail 3
```

Narration: “Lev advises rather than authorizes. High-confidence read actions are narrowed; uncertain cases preserve Loop’s full toolset, and missing critical context triggers clarification. Existing approval checks still control any write, edit, or shell action.”

If Lev latency is visible, mention it plainly: the evaluation found roughly 25–31 seconds per decision, so per-turn caching is the next performance fix.
