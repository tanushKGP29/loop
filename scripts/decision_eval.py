#!/usr/bin/env python3
"""Evaluate Loop decision workflows without exposing human labels to a model.

The input JSONL is deliberately human-labelled. Each model sees only the user
prompt and fixed tool/task choices; this script never sends `labels` over the
network. It uses only the Python standard library so it can run in the same
environment as the local Lev server.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import sys
import time
import urllib.error
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Any, Callable

TASK_TYPES = ("answer", "clarify", "execute", "inspect", "modify")
# Human labels may use the more descriptive Phase 6 names below. Loop itself
# only sends/accepts the five canonical names in TASK_TYPES.
TASK_TYPE_ALIASES = {
    "answer": "answer",
    "clarify": "clarify",
    "execute": "execute",
    "inspect": "inspect",
    "modify": "modify",
    "general_qa": "answer",
    "investigate": "inspect",
    "code_edit": "modify",
    "run_command": "execute",
    # Web search/fetch is an external action in the current Loop taxonomy.
    "web_research": "execute",
}
BANDS = ((0.0, 0.5), (0.5, 0.7), (0.7, 0.9), (0.9, 1.000001))
THRESHOLDS = (0.50, 0.60, 0.70, 0.75, 0.80, 0.90, 0.95)


class ModelError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=Path, required=True, help="Human-labelled JSONL input.")
    parser.add_argument("--out-dir", type=Path, required=True, help="Directory for raw results and report.")
    parser.add_argument("--tool-catalog", type=Path, default=Path("evals/decision/tool_catalog.json"))
    parser.add_argument("--lev-base-url", default=os.getenv("LEV_BASE_URL", "http://127.0.0.1:8000"))
    parser.add_argument("--lev-api-key", default=os.getenv("LEV_API_KEY", "local"))
    parser.add_argument("--lev-model", default=os.getenv("LEV_MODEL"))
    parser.add_argument("--soket-base-url", default=os.getenv("SOKET_BASE_URL", "https://api.soket.ai/v1"))
    parser.add_argument("--soket-api-key-env", default="SOKET_API_KEY")
    parser.add_argument(
        "--use-loop-soket-auth",
        action="store_true",
        help="Read the existing local Loop Soket credential in memory; never prints or stores the key in results.",
    )
    parser.add_argument(
        "--loop-auth-path",
        type=Path,
        default=Path.home() / ".loop" / "agent" / "auth.json",
        help="Loop auth.json path used only with --use-loop-soket-auth.",
    )
    parser.add_argument("--soket-model", help="Soket chat-completions model id.")
    parser.add_argument("--open-systemone-name", default=os.getenv("OPEN_SYSTEMONE_NAME", "laya"))
    parser.add_argument("--open-systemone-base-url", default=os.getenv("LAYA_BASE_URL"))
    parser.add_argument("--open-systemone-api-key-env", default=os.getenv("OPEN_SYSTEMONE_API_KEY_ENV", "LAYA_API_KEY"))
    parser.add_argument("--open-systemone-model", default=os.getenv("OPEN_SYSTEMONE_MODEL"))
    parser.add_argument("--skip-lev", action="store_true")
    parser.add_argument("--skip-soket", action="store_true")
    parser.add_argument("--skip-open-systemone", action="store_true")
    parser.add_argument("--timeout-s", type=float, default=120.0)
    parser.add_argument("--limit", type=int, help="Run only the first N labelled cases (pilot use only).")
    parser.add_argument("--resume", action="store_true", help="Resume from an existing results.jsonl in --out-dir.")
    parser.add_argument(
        "--retry-errors",
        action="store_true",
        help="With --resume, retry only cases whose latest recorded attempt failed.",
    )
    parser.add_argument("--dry-run", action="store_true", help="Validate labels and planned models without making requests.")
    return parser.parse_args()


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"{path}:{number}: invalid JSON: {exc}") from exc
        rows.append(row)
    if not rows:
        raise SystemExit(f"{path}: no cases found")
    return rows


def validate_labels(cases: list[dict[str, Any]], tool_names: set[str]) -> None:
    errors: list[str] = []
    seen: set[str] = set()
    for row in cases:
        case_id = str(row.get("id", "<missing id>"))
        if case_id in seen:
            errors.append(f"{case_id}: duplicate id")
        seen.add(case_id)
        labels = row.get("labels")
        if not isinstance(labels, dict):
            errors.append(f"{case_id}: labels must be an object")
            continue
        tools = labels.get("acceptable_tools")
        if not isinstance(tools, list) or not tools or not all(isinstance(tool, str) for tool in tools):
            errors.append(f"{case_id}: acceptable_tools must be a non-empty list")
        elif unknown := set(tools) - tool_names:
            errors.append(f"{case_id}: unknown acceptable tool(s): {', '.join(sorted(unknown))}")
        if labels.get("task_type") not in TASK_TYPE_ALIASES:
            errors.append(
                f"{case_id}: task_type must be one of {', '.join(sorted(TASK_TYPE_ALIASES))}"
            )
        if not isinstance(labels.get("critical_context_missing"), bool):
            errors.append(f"{case_id}: critical_context_missing must be true or false")
        if row.get("split") not in {"tuning", "holdout"}:
            errors.append(f"{case_id}: split must be tuning or holdout")
        if not isinstance(row.get("prompt"), str) or not row["prompt"].strip():
            errors.append(f"{case_id}: prompt must be non-empty text")
    if errors:
        shown = "\n  ".join(errors[:25])
        suffix = "\n  ..." if len(errors) > 25 else ""
        raise SystemExit(
            "Labels are incomplete or invalid. Review them yourself before running any model:\n  "
            + shown
            + suffix
        )


def endpoint(base_url: str, suffix: str) -> str:
    base = base_url.rstrip("/")
    return f"{base}{suffix}" if base.endswith("/v1") else f"{base}/v1{suffix}"


def post_json(url: str, payload: dict[str, Any], api_key: str | None, timeout_s: float) -> dict[str, Any]:
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout_s) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")[:300]
        raise ModelError(f"HTTP {exc.code}: {body}") from exc
    except urllib.error.URLError as exc:
        raise ModelError(f"network error: {exc.reason}") from exc
    try:
        decoded = json.loads(body)
    except json.JSONDecodeError as exc:
        raise ModelError(f"invalid JSON response: {body[:300]}") from exc
    if not isinstance(decoded, dict):
        raise ModelError("response root must be an object")
    return decoded


def read_loop_soket_key(path: Path) -> str:
    """Load only Loop's stored Soket key without exposing it in output/files."""
    try:
        auth = json.loads(path.read_text(encoding="utf-8"))
        credential = auth["providers"]["soket"]
        key = credential["key"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as exc:
        raise SystemExit(f"Could not read a stored Soket credential from {path}") from exc
    if credential.get("type") != "apiKey" or not isinstance(key, str) or not key.strip():
        raise SystemExit(f"No usable stored Soket API-key credential found in {path}")
    return key


def systemone_payload(prompt: str, tools: dict[str, str], model: str | None) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "state": {
            "latest_user_message": prompt,
            "available_tool_names": list(tools),
        },
        "questions": {
            "tool": {
                "type": "choice",
                "instructions": "Which tool should run next? Choose none when the request can be answered directly.",
                "criteria": tools,
            },
            "task_type": {
                "type": "choice",
                "instructions": "Classify the user's request.",
                "criteria": {
                    "answer": "answer directly without repository investigation",
                    "clarify": "ask for required missing information",
                    "execute": "run a command or verification",
                    "inspect": "read or investigate without modifying files",
                    "modify": "create or change files",
                },
            },
            "critical_context_missing": {
                "type": "noul",
                "instructions": "Is critical context missing such that Loop should ask a clarifying question before acting?",
            },
        },
    }
    if model:
        payload["model"] = model
    return payload


def as_probability(value: Any, field: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool) or not math.isfinite(value):
        raise ModelError(f"{field} must be a finite number")
    return min(1.0, max(0.0, float(value)))


def parse_systemone(response: dict[str, Any]) -> dict[str, Any]:
    answers = response.get("answers")
    if not isinstance(answers, dict):
        raise ModelError("System One response has no answers object")
    tool = answers.get("tool")
    task = answers.get("task_type")
    missing = answers.get("critical_context_missing")
    if not all(isinstance(answer, dict) for answer in (tool, task, missing)):
        raise ModelError("System One response omitted a required answer")
    tool_value = tool.get("choice", tool.get("value"))
    task_value = task.get("choice", task.get("value"))
    probability_yes = missing.get("noul", missing.get("probability_yes"))
    if not isinstance(tool_value, str) or not isinstance(task_value, str):
        raise ModelError("System One choice answer has no string value")
    noul_probability = as_probability(probability_yes, "critical_context_missing.noul")
    return {
        "tool": tool_value,
        "task_type": task_value,
        "critical_context_missing": noul_probability >= 0.5,
        "confidence": {
            "tool": as_probability(tool.get("confidence"), "tool.confidence"),
            "task_type": as_probability(task.get("confidence"), "task_type.confidence"),
            "critical_context_missing": abs(2.0 * noul_probability - 1.0),
        },
        "model": response.get("model"),
    }


def predict_systemone(
    base_url: str, api_key: str | None, model: str | None, timeout_s: float, tools: dict[str, str]
) -> Callable[[str], dict[str, Any]]:
    url = endpoint(base_url, "/systemone")

    def predict(prompt: str) -> dict[str, Any]:
        response = post_json(url, systemone_payload(prompt, tools, model), api_key, timeout_s)
        return parse_systemone(response)

    return predict


def parse_json_content(content: str) -> dict[str, Any]:
    text = content.strip()
    if text.startswith("```"):
        text = text.split("\n", 1)[1] if "\n" in text else ""
        text = text.rsplit("```", 1)[0].strip()
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError as exc:
        raise ModelError(f"plain LLM did not return a JSON object: {text[:300]}") from exc
    if not isinstance(parsed, dict):
        raise ModelError("plain LLM response must be a JSON object")
    return parsed


def predict_plain_llm(
    base_url: str, api_key: str, model: str, timeout_s: float, tools: dict[str, str]
) -> Callable[[str], dict[str, Any]]:
    url = endpoint(base_url, "/chat/completions")
    system = (
        "You are a decision baseline for a coding-agent benchmark. Return exactly one JSON object, no prose or markdown. "
        "Select one allowed tool, one task_type, and whether critical context is missing before acting. "
        "Confidence values are your own probabilities from 0 to 1; do not claim calibration. "
        f"Allowed tools: {json.dumps(tools, ensure_ascii=False)}. "
        f"Allowed task_type values: {list(TASK_TYPES)}. "
        "Schema: {\"tool\": string, \"task_type\": string, \"critical_context_missing\": boolean, "
        "\"confidence\": {\"tool\": number, \"task_type\": number, \"critical_context_missing\": number}}."
    )

    def predict(prompt: str) -> dict[str, Any]:
        response = post_json(
            url,
            {
                "model": model,
                "temperature": 0,
                "stream": False,
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
            },
            api_key,
            timeout_s,
        )
        try:
            content = response["choices"][0]["message"]["content"]
        except (KeyError, IndexError, TypeError) as exc:
            raise ModelError("chat-completions response has no message content") from exc
        parsed = parse_json_content(content)
        confidence = parsed.get("confidence")
        if not isinstance(confidence, dict):
            raise ModelError("plain LLM response has no confidence object")
        tool = parsed.get("tool")
        task_type = parsed.get("task_type")
        context_missing = parsed.get("critical_context_missing")
        if not isinstance(tool, str) or not isinstance(task_type, str) or not isinstance(context_missing, bool):
            raise ModelError("plain LLM response has invalid decision fields")
        return {
            "tool": tool,
            "task_type": task_type,
            "critical_context_missing": context_missing,
            "confidence": {
                "tool": as_probability(confidence.get("tool"), "confidence.tool"),
                "task_type": as_probability(confidence.get("task_type"), "confidence.task_type"),
                "critical_context_missing": as_probability(
                    confidence.get("critical_context_missing"), "confidence.critical_context_missing"
                ),
            },
            "model": response.get("model", model),
        }

    return predict


def score(labels: dict[str, Any], prediction: dict[str, Any]) -> dict[str, bool]:
    return {
        "tool": prediction["tool"] in labels["acceptable_tools"],
        "task_type": prediction["task_type"] == TASK_TYPE_ALIASES[labels["task_type"]],
        "critical_context_missing": prediction["critical_context_missing"] == labels["critical_context_missing"],
    }


def percentile(values: list[float], p: float) -> float | None:
    if not values:
        return None
    sorted_values = sorted(values)
    index = min(len(sorted_values) - 1, math.ceil(p * len(sorted_values)) - 1)
    return sorted_values[index]


def reliability(rows: list[dict[str, Any]], field: str) -> list[dict[str, Any]]:
    output = []
    for low, high in BANDS:
        selected = [row for row in rows if low <= row["prediction"]["confidence"][field] < high]
        if not selected:
            continue
        confidences = [row["prediction"]["confidence"][field] for row in selected]
        output.append(
            {
                "band": f"[{low:.2f}, {min(high, 1.0):.2f}{']' if high > 1 else ')'}",
                "n": len(selected),
                "mean_confidence": round(statistics.fmean(confidences), 4),
                "accuracy": round(statistics.fmean(row["scores"][field] for row in selected), 4),
            }
        )
    return output


def summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    successful = [row for row in rows if row.get("prediction")]
    failures = len(rows) - len(successful)
    if not successful:
        return {"n": 0, "failures": failures}
    fields = ("tool", "task_type", "critical_context_missing")
    accuracy = {field: statistics.fmean(row["scores"][field] for row in successful) for field in fields}
    joint = statistics.fmean(all(row["scores"].values()) for row in successful)
    latency = [row["latency_ms"] for row in successful]
    threshold_table = []
    for threshold in THRESHOLDS:
        gated = [row for row in successful if row["prediction"]["confidence"]["tool"] >= threshold]
        threshold_table.append(
            {
                "threshold": threshold,
                "coverage": round(len(gated) / len(successful), 4),
                "n": len(gated),
                "tool_accuracy": round(statistics.fmean(row["scores"]["tool"] for row in gated), 4) if gated else None,
            }
        )
    return {
        "n": len(successful),
        "failures": failures,
        "accuracy": {key: round(value, 4) for key, value in accuracy.items()},
        "joint_accuracy": round(joint, 4),
        "latency_ms": {
            "mean": round(statistics.fmean(latency), 1),
            "p50": percentile(latency, 0.50),
            "p95": percentile(latency, 0.95),
        },
        "reliability": {field: reliability(successful, field) for field in fields},
        "tool_thresholds": threshold_table,
    }


def latest_attempts(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Keep the last attempt per model/case when transient failures are retried."""
    latest: dict[tuple[str, str], dict[str, Any]] = {}
    for row in rows:
        latest[(str(row.get("model")), str(row.get("case_id")))] = row
    return list(latest.values())


def write_report(path: Path, summary: dict[str, dict[str, dict[str, Any]]]) -> None:
    lines = ["# Decision-model evaluation", "", "Human labels were validated before requests were sent.", ""]
    for model, by_split in summary.items():
        lines.extend([f"## {model}", ""])
        for split, result in by_split.items():
            lines.extend([f"### {split}", ""])
            if not result["n"]:
                lines.extend([f"No successful cases (failures: {result['failures']}).", ""])
                continue
            accuracy = result["accuracy"]
            latency = result["latency_ms"]
            lines.extend(
                [
                    "| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |",
                    "| ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
                    f"| {result['n']} | {accuracy['tool']:.1%} | {accuracy['task_type']:.1%} | {accuracy['critical_context_missing']:.1%} | {result['joint_accuracy']:.1%} | {latency['mean']:.1f} ms | {latency['p95']} ms |",
                    "",
                    "#### Tool reliability by confidence band",
                    "",
                    "| band | cases | mean confidence | empirical accuracy |",
                    "| --- | ---: | ---: | ---: |",
                ]
            )
            for band in result["reliability"]["tool"]:
                lines.append(f"| {band['band']} | {band['n']} | {band['mean_confidence']:.1%} | {band['accuracy']:.1%} |")
            lines.extend(["", "#### Tool threshold candidates", "", "| threshold | coverage | cases | tool accuracy |", "| ---: | ---: | ---: | ---: |"])
            for row in result["tool_thresholds"]:
                accuracy_text = "—" if row["tool_accuracy"] is None else f"{row['tool_accuracy']:.1%}"
                lines.append(f"| {row['threshold']:.2f} | {row['coverage']:.1%} | {row['n']} | {accuracy_text} |")
            lines.append("")
    lines.extend(
        [
            "## Interpretation guardrails",
            "",
            "- Select a production threshold from the tuning rows, then report the held-out result unchanged.",
            "- Do not claim calibration from a sparse band. Each reported band should have enough examples to be meaningful.",
            "- The Soket baseline returns self-reported confidence through prompted JSON; compare its accuracy and latency, but do not treat its confidence as calibrated like a System One probability.",
        ]
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()
    cases = load_jsonl(args.cases)
    tools = json.loads(args.tool_catalog.read_text(encoding="utf-8"))
    if not isinstance(tools, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in tools.items()):
        raise SystemExit("tool catalog must be an object of string names to string descriptions")
    validate_labels(cases, set(tools))
    if args.limit:
        cases = cases[: args.limit]

    predictors: dict[str, Callable[[str], dict[str, Any]]] = {}
    if not args.skip_lev:
        predictors["lev"] = predict_systemone(
            args.lev_base_url, args.lev_api_key, args.lev_model, args.timeout_s, tools
        )
    if not args.skip_soket:
        key = os.getenv(args.soket_api_key_env)
        if not key and args.use_loop_soket_auth:
            key = read_loop_soket_key(args.loop_auth_path)
        if not key:
            raise SystemExit(
                f"{args.soket_api_key_env} is required unless --skip-soket is set "
                "or --use-loop-soket-auth is supplied"
            )
        if not args.soket_model:
            raise SystemExit("--soket-model is required unless --skip-soket is set")
        predictors["soket_plain_llm"] = predict_plain_llm(
            args.soket_base_url, key, args.soket_model, args.timeout_s, tools
        )
    if not args.skip_open_systemone:
        if not args.open_systemone_base_url:
            raise SystemExit("--open-systemone-base-url or LAYA_BASE_URL is required unless --skip-open-systemone is set")
        open_key = os.getenv(args.open_systemone_api_key_env)
        predictors[args.open_systemone_name] = predict_systemone(
            args.open_systemone_base_url, open_key, args.open_systemone_model, args.timeout_s, tools
        )

    print(f"Validated {len(cases)} human-labelled cases; models: {', '.join(predictors)}")
    if args.dry_run:
        return 0

    args.out_dir.mkdir(parents=True, exist_ok=True)
    raw_path = args.out_dir / "results.jsonl"
    all_rows: list[dict[str, Any]] = []
    completed: set[tuple[str, str]] = set()
    if raw_path.exists():
        if not args.resume:
            raise SystemExit(f"{raw_path} already exists; choose a new --out-dir or pass --resume")
        all_rows = load_jsonl(raw_path)
        latest = latest_attempts(all_rows)
        completed = {
            (str(row.get("model")), str(row.get("case_id")))
            for row in latest
            if not (args.retry_errors and row.get("error"))
        }
        print(f"Resuming with {len(all_rows)} existing result row(s).")
    with raw_path.open("a", encoding="utf-8", newline="\n") as raw_file:
        for model_name, predict in predictors.items():
            for index, case in enumerate(cases, start=1):
                key = (model_name, case["id"])
                if key in completed:
                    print(f"[{model_name}] {index}/{len(cases)} {case['id']} (already recorded)", flush=True)
                    continue
                print(f"[{model_name}] {index}/{len(cases)} {case['id']}", flush=True)
                started = time.perf_counter()
                row: dict[str, Any] = {"model": model_name, "case_id": case["id"], "split": case["split"]}
                try:
                    prediction = predict(case["prompt"])
                    row["prediction"] = prediction
                    row["expected"] = {
                        "acceptable_tools": case["labels"]["acceptable_tools"],
                        "task_type": TASK_TYPE_ALIASES[case["labels"]["task_type"]],
                        "critical_context_missing": case["labels"]["critical_context_missing"],
                    }
                    row["scores"] = score(case["labels"], prediction)
                except ModelError as exc:
                    row["error"] = str(exc)
                row["latency_ms"] = round((time.perf_counter() - started) * 1000)
                raw_file.write(json.dumps(row, ensure_ascii=False) + "\n")
                raw_file.flush()
                os.fsync(raw_file.fileno())
                all_rows.append(row)
    final_rows = latest_attempts(all_rows)
    grouped: dict[str, dict[str, list[dict[str, Any]]]] = defaultdict(lambda: defaultdict(list))
    for row in final_rows:
        grouped[row["model"]][row["split"]].append(row)
    summary = {model: {split: summarize(rows) for split, rows in by_split.items()} for model, by_split in grouped.items()}
    (args.out_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    write_report(args.out_dir / "report.md", summary)
    print(f"Wrote {raw_path}, {args.out_dir / 'summary.json'}, and {args.out_dir / 'report.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
