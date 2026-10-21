"""Summarize raw native application frame pacing; no GPU-time inference."""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import statistics
from typing import Any


def summarize(report: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(report, dict):
        raise ValueError("frame capture must be a JSON object")
    if report.get("format") != "ancient-earth-frame-capture-v1":
        raise ValueError("unsupported frame-capture format")
    intervals = report.get("interval_seconds")
    if not isinstance(intervals, list) or not intervals:
        raise ValueError("a nonempty interval_seconds array is required")
    if any(
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(value)
        or value <= 0
        for value in intervals
    ):
        raise ValueError("all intervals must be finite positive seconds")
    count = len(intervals)
    if type(report.get("sample_count")) is not int or report["sample_count"] != count:
        raise ValueError("sample_count disagrees with the raw data")
    duration = math.fsum(intervals)
    if not math.isfinite(duration):
        raise ValueError("recorded duration overflows")
    declared = report.get("recorded_duration_seconds")
    if (
        isinstance(declared, bool)
        or not isinstance(declared, (int, float))
        or not math.isfinite(declared)
        or not math.isclose(declared, duration, rel_tol=1e-10, abs_tol=1e-9)
    ):
        raise ValueError("recorded duration disagrees with raw intervals")
    ordered = sorted(intervals)
    # Nearest-rank percentile: ceil(p*n) in one-based notation. Integer
    # arithmetic avoids roundoff at exact 95th/99th rank boundaries.
    percentile = lambda p: ordered[(p * count + 99) // 100 - 1] * 1000.0
    throughput = count / duration
    timing = {
        "minimum": ordered[0] * 1000.0,
        "mean": duration / count * 1000.0,
        "median": statistics.median(ordered) * 1000.0,
        "p95_nearest_rank": percentile(95),
        "p99_nearest_rank": percentile(99),
        "maximum": ordered[-1] * 1000.0,
    }
    if not math.isfinite(throughput) or any(not math.isfinite(v) for v in timing.values()):
        raise ValueError("derived frame statistics overflow")
    return {
        "measurement": "application frame pacing on the recorded machine/configuration",
        "not_measured": "GPU execution time, latency, or hardware-independent performance",
        "metadata": report.get("metadata"),
        "view_changed_during_capture": report.get("view_changed_during_capture"),
        "incomplete_scene_frames": report.get("incomplete_scene_frames"),
        "motion_qa": report.get("motion_qa"),
        "eligible_as_stationary_scene_baseline": (
            report.get("motion_qa") is None
            and report.get("view_changed_during_capture") is False
            and type(report.get("incomplete_scene_frames")) is int
            and report["incomplete_scene_frames"] == 0
            and type(report.get("unfocused_recorded_frames")) is int
            and report["unfocused_recorded_frames"] == 0
        ),
        "unfocused_recorded_frames": report.get("unfocused_recorded_frames"),
        "sample_count": count,
        "recorded_duration_seconds": duration,
        "throughput_frames_per_second": throughput,
        "frame_interval_ms": timing,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("captures", type=Path, nargs="+")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    summaries = []
    try:
        for path in args.captures:
            report = json.loads(path.read_text(encoding="utf-8"))
            summaries.append({"capture": str(path), **summarize(report)})
    except (OSError, ValueError, TypeError, OverflowError) as error:
        parser.error(str(error))
    text = json.dumps(summaries, indent=2, allow_nan=False) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text, encoding="utf-8")
    print(text, end="")


if __name__ == "__main__":
    main()
