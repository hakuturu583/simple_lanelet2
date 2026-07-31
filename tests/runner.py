#!/usr/bin/env python3
"""Cross-implementation diff harness.

Every case in ``tests/cases`` is executed three ways and the resulting JSON Lines
streams are compared:

===========  =======================  =============================================
run          interpreter              environment
===========  =======================  =============================================
``REF``      ``.venv-ref/bin/python``  the real ``lanelet2==1.2.3`` from PyPI
``COMPAT``   ``.venv/bin/python``      ours, with ``LANELET2_BUG_COMPAT=1``
``FIXED``    ``.venv/bin/python``      ours, default (upstream defects repaired)
===========  =======================  =============================================

Two independent assertions come out of that:

1. **REF vs COMPAT must match exactly.** This is the compatibility claim. The only
   permitted differences are the genuinely out-of-scope items listed in
   ``tests/divergence.toml``, each of which needs a stated reason.
2. **COMPAT vs FIXED must differ in exactly the places listed in
   ``tests/compat_matrix.toml``** -- no more (an unintended behaviour change) and no
   fewer (a repair that is not actually wired up). The table is checked in both
   directions, so it cannot rot.

Run under Python 3.11+ (it needs ``tomllib``); ``just diff`` invokes it with
``.venv/bin/python``. It only uses the standard library and drives the two
interpreters through ``subprocess``, so it belongs to neither environment.
"""

from __future__ import annotations

import argparse
import difflib
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from fnmatch import fnmatch
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES_DIR = ROOT / "tests" / "cases"
LIB_DIR = ROOT / "tests" / "lib"
DATA_DIR = ROOT / "tests" / "data"
REF_PYTHON = ROOT / ".venv-ref" / "bin" / "python"
OUR_PYTHON = ROOT / ".venv" / "bin" / "python"

DEFAULT_REL_TOL = 1e-12
DEFAULT_ABS_TOL = 1e-12
CASE_TIMEOUT = 300

TOL_RE = re.compile(r"^#\s*TOL:\s*(.*)$", re.MULTILINE)
EXPECT_RE = re.compile(r"^#\s*EXPECT:\s*(\w+)\s*$", re.MULTILINE)

#: Iterating a map layer without sorting is the single most common source of
#: flaky cases, because upstream layers are ``std::unordered_map``. Catch it
#: statically rather than debugging it later.
LAYER_ITER_RE = re.compile(
    r"(?<!by_id\()(?:for\s+\w+\s+in\s+|list\(|sorted\()\s*[\w.]*\.(?:point|lineString|polygon|lanelet|area|regulatoryElement)Layer\b"
)

RESET = "\033[0m"
RED = "\033[31m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
DIM = "\033[2m"


def colour(text: str, code: str) -> str:
    return text if not sys.stdout.isatty() else f"{code}{text}{RESET}"


# --------------------------------------------------------------------------
# configuration ledgers
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Rule:
    case: str
    key: str
    reason: str

    def matches(self, case: str, key: str) -> bool:
        return fnmatch(case, self.case) and fnmatch(key, self.key)


def load_rules(path: Path, table: str) -> list[Rule]:
    if not path.exists():
        return []
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    rules = []
    for entry in data.get(table, []):
        missing = {"case", "key", "reason"} - entry.keys()
        if missing:
            raise SystemExit(f"{path}: [[{table}]] entry is missing {sorted(missing)}")
        if not entry["reason"].strip():
            raise SystemExit(f"{path}: [[{table}]] entry for {entry['case']}/{entry['key']} has an empty reason")
        rules.append(Rule(entry["case"], entry["key"], entry["reason"]))
    return rules


# --------------------------------------------------------------------------
# running a case
# --------------------------------------------------------------------------


@dataclass
class Record:
    key: str
    value: object


@dataclass
class RunResult:
    returncode: int
    records: list[Record]
    stderr: str
    parse_error: str | None = None

    def as_dict(self) -> dict[str, object]:
        return {record.key: record.value for record in self.records}


def run_case(case: Path, python: Path, extra_env: dict[str, str]) -> RunResult:
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": os.environ.get("HOME", ""),
        "PYTHONPATH": str(LIB_DIR),
        "PYTHONHASHSEED": "0",
        "PYTHONDONTWRITEBYTECODE": "1",
        "LC_ALL": "C",
        "TZ": "UTC",
        "LANELET2_DATA": str(DATA_DIR),
    }
    env.update(extra_env)

    with tempfile.TemporaryDirectory(prefix="ll2-case-") as workdir:
        try:
            proc = subprocess.run(
                [str(python), str(case)],
                env=env,
                cwd=workdir,
                capture_output=True,
                text=True,
                timeout=CASE_TIMEOUT,
            )
        except subprocess.TimeoutExpired:
            return RunResult(-1, [], f"timed out after {CASE_TIMEOUT}s")

    records: list[Record] = []
    parse_error = None
    for lineno, line in enumerate(proc.stdout.splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
            records.append(Record(obj["k"], obj["v"]))
        except (json.JSONDecodeError, KeyError, TypeError) as exc:
            parse_error = f"line {lineno}: {exc}: {line[:200]!r}"
            break
    return RunResult(proc.returncode, records, proc.stderr, parse_error)


# --------------------------------------------------------------------------
# value comparison
# --------------------------------------------------------------------------


@dataclass
class Deviation:
    rel: float = 0.0
    abs: float = 0.0

    def observe(self, a: float, b: float) -> None:
        diff = abs(a - b)
        if math.isfinite(diff):
            self.abs = max(self.abs, diff)
            scale = max(abs(a), abs(b))
            if scale > 0:
                self.rel = max(self.rel, diff / scale)


def values_equal(a: object, b: object, rel_tol: float, abs_tol: float, dev: Deviation) -> bool:
    if isinstance(a, bool) or isinstance(b, bool):
        return a is b
    if isinstance(a, float) or isinstance(b, float):
        if not isinstance(a, (int, float)) or not isinstance(b, (int, float)):
            return False
        if math.isnan(a) and math.isnan(b):
            return True
        if math.isinf(a) or math.isinf(b):
            return a == b
        dev.observe(float(a), float(b))
        return math.isclose(a, b, rel_tol=rel_tol, abs_tol=abs_tol)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(
            values_equal(x, y, rel_tol, abs_tol, dev) for x, y in zip(a, b)
        )
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(
            values_equal(a[k], b[k], rel_tol, abs_tol, dev) for k in a
        )
    return type(a) is type(b) and a == b


def coverage_of(ours: object, ref: object) -> float:
    """How close ``ours`` is to ``ref``, for progress reporting on partial cases."""
    if ours == ref:
        return 1.0
    if isinstance(ours, list) and isinstance(ref, list):
        ref_set = set(map(repr, ref))
        if not ref_set:
            return 1.0
        return len(ref_set & set(map(repr, ours))) / len(ref_set)
    return 0.0


# --------------------------------------------------------------------------
# comparing two runs
# --------------------------------------------------------------------------


@dataclass
class Comparison:
    differing: list[str] = field(default_factory=list)
    details: dict[str, str] = field(default_factory=dict)
    deviation: Deviation = field(default_factory=Deviation)
    structural: list[str] = field(default_factory=list)


def render(value: object, width: int = 300) -> str:
    text = json.dumps(value, sort_keys=True)
    return text if len(text) <= width else text[:width] + "..."


def diff_runs(left: RunResult, right: RunResult, rel_tol: float, abs_tol: float) -> Comparison:
    """Compare two runs. ``left`` is the authority (REF, or COMPAT)."""
    result = Comparison()

    if left.parse_error or right.parse_error:
        result.structural.append(
            f"unparseable output (left: {left.parse_error}, right: {right.parse_error})"
        )
    if left.returncode != right.returncode:
        result.structural.append(f"exit code {left.returncode} vs {right.returncode}")

    left_keys = [record.key for record in left.records]
    right_keys = [record.key for record in right.records]
    if left_keys != right_keys:
        only_left = [k for k in left_keys if k not in set(right_keys)]
        only_right = [k for k in right_keys if k not in set(left_keys)]
        if only_left:
            result.structural.append(f"missing keys: {only_left[:20]}")
        if only_right:
            result.structural.append(f"unexpected keys: {only_right[:20]}")
        if not only_left and not only_right:
            result.structural.append("keys emitted in a different order")

    left_map, right_map = left.as_dict(), right.as_dict()
    for key in left_map:
        if key not in right_map:
            continue
        if not values_equal(left_map[key], right_map[key], rel_tol, abs_tol, result.deviation):
            result.differing.append(key)
            result.details[key] = format_mismatch(left_map[key], right_map[key])
    return result


def format_mismatch(left: object, right: object) -> str:
    if isinstance(left, str) and isinstance(right, str) and ("\n" in left or "\n" in right):
        lines = difflib.unified_diff(
            left.splitlines(), right.splitlines(), "left", "right", lineterm="", n=1
        )
        return "\n".join(list(lines)[:40])
    return f"left={render(left)}\n      right={render(right)}"


# --------------------------------------------------------------------------
# per-case driver
# --------------------------------------------------------------------------


@dataclass
class CaseOutcome:
    name: str
    status: str  # ok | fail | partial | skipped
    messages: list[str] = field(default_factory=list)
    coverage: float | None = None
    deviation: Deviation = field(default_factory=Deviation)


def parse_directives(source: str) -> tuple[float, float, str]:
    rel_tol, abs_tol = DEFAULT_REL_TOL, DEFAULT_ABS_TOL
    match = TOL_RE.search(source)
    if match:
        for token in match.group(1).split():
            name, _, value = token.partition("=")
            if name == "rel":
                rel_tol = float(value)
            elif name == "abs":
                abs_tol = float(value)
    expect = EXPECT_RE.search(source)
    return rel_tol, abs_tol, (expect.group(1) if expect else "exact")


def lint_case(source: str) -> list[str]:
    problems = []
    for match in LAYER_ITER_RE.finditer(source):
        line = source[: match.start()].count("\n") + 1
        problems.append(
            f"line {line}: layer iteration without by_id(); layer order is not reproducible"
        )
    return problems


def check_case(case: Path, mode: str, divergences: list[Rule], compat_matrix: list[Rule]) -> CaseOutcome:
    name = case.stem
    source = case.read_text(encoding="utf-8")
    rel_tol, abs_tol, expect = parse_directives(source)

    lint_problems = lint_case(source)
    if lint_problems:
        return CaseOutcome(name, "fail", [f"lint: {p}" for p in lint_problems])

    outcome = CaseOutcome(name, "ok")

    ref = run_case(case, REF_PYTHON, {})
    compat = run_case(case, OUR_PYTHON, {"LANELET2_BUG_COMPAT": "1"})

    # --- assertion 1: REF vs COMPAT ---------------------------------------
    cmp1 = diff_runs(ref, compat, rel_tol, abs_tol)
    outcome.deviation = cmp1.deviation

    unexplained = [
        key
        for key in cmp1.differing
        if not any(rule.matches(name, key) for rule in divergences)
    ]

    if expect == "partial":
        ref_map, compat_map = ref.as_dict(), compat.as_dict()
        scores = [coverage_of(compat_map.get(key), value) for key, value in ref_map.items()]
        outcome.coverage = sum(scores) / len(scores) if scores else 1.0
        outcome.status = "partial"
        if compat.parse_error:
            outcome.status = "fail"
            outcome.messages.append(f"COMPAT produced unparseable output: {compat.parse_error}")
    else:
        if cmp1.structural:
            outcome.status = "fail"
            outcome.messages += [f"REF vs COMPAT: {m}" for m in cmp1.structural]
            if compat.stderr.strip():
                outcome.messages.append("COMPAT stderr:\n" + indent(compat.stderr.strip()))
            if ref.stderr.strip():
                outcome.messages.append("REF stderr:\n" + indent(ref.stderr.strip()))
        for key in unexplained:
            outcome.status = "fail"
            outcome.messages.append(f"REF vs COMPAT differs at {key!r}:\n      {cmp1.details[key]}")

    # A divergence rule that no longer fires means the ledger has rotted.
    for rule in divergences:
        if fnmatch(name, rule.case):
            fired = any(rule.matches(name, key) for key in cmp1.differing)
            declared = any(fnmatch(key, rule.key) for key in ref.as_dict())
            if declared and not fired and expect != "partial":
                outcome.status = "fail"
                outcome.messages.append(
                    f"divergence.toml entry {rule.case}/{rule.key} no longer differs -- remove it"
                )

    if mode == "compat":
        return outcome

    # --- assertion 2: COMPAT vs FIXED -------------------------------------
    fixed = run_case(case, OUR_PYTHON, {})
    cmp2 = diff_runs(compat, fixed, rel_tol, abs_tol)

    if cmp2.structural:
        outcome.status = "fail"
        outcome.messages += [f"COMPAT vs FIXED: {m}" for m in cmp2.structural]

    expected_keys = {
        key
        for key in compat.as_dict()
        if any(rule.matches(name, key) for rule in compat_matrix)
    }
    actual_keys = set(cmp2.differing)

    for key in sorted(actual_keys - expected_keys):
        outcome.status = "fail"
        outcome.messages.append(
            f"unintended behaviour change at {key!r} (not in compat_matrix.toml):\n      {cmp2.details[key]}"
        )
    for key in sorted(expected_keys - actual_keys):
        outcome.status = "fail"
        outcome.messages.append(
            f"compat_matrix.toml claims {key!r} is repaired, but COMPAT and FIXED agree"
        )

    return outcome


def indent(text: str, prefix: str = "      ") -> str:
    return "\n".join(prefix + line for line in text.splitlines())


# --------------------------------------------------------------------------
# entry point
# --------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("-k", "--filter", help="only run cases whose name contains this")
    parser.add_argument(
        "--mode",
        choices=["all", "compat"],
        default="all",
        help="'compat' skips the COMPAT-vs-FIXED assertion (faster dev loop)",
    )
    parser.add_argument("--show", metavar="CASE", help="print both streams for one case")
    parser.add_argument("--coverage", action="store_true", help="only report partial-case coverage")
    args = parser.parse_args()

    for python, label in ((REF_PYTHON, ".venv-ref"), (OUR_PYTHON, ".venv")):
        if not python.exists():
            print(f"{label} is missing -- run `just venvs` first", file=sys.stderr)
            return 2

    divergences = load_rules(ROOT / "tests" / "divergence.toml", "allow")
    compat_matrix = load_rules(ROOT / "tests" / "compat_matrix.toml", "entry")

    cases = sorted(CASES_DIR.glob("*.py"))
    if args.show:
        cases = [c for c in cases if args.show in c.stem]
        return show(cases[0]) if cases else 2
    if args.filter:
        cases = [c for c in cases if args.filter in c.stem]
    if not cases:
        print("no cases matched", file=sys.stderr)
        return 2

    outcomes = [check_case(case, args.mode, divergences, compat_matrix) for case in cases]

    failures = 0
    for outcome in outcomes:
        if args.coverage and outcome.status != "partial":
            continue
        if outcome.status == "ok":
            note = ""
            if outcome.deviation.abs > 0:
                note = colour(
                    f"  (max dev abs={outcome.deviation.abs:.3g} rel={outcome.deviation.rel:.3g})",
                    DIM,
                )
            print(f"{colour('PASS', GREEN)}  {outcome.name}{note}")
        elif outcome.status == "partial":
            pct = (outcome.coverage or 0.0) * 100
            print(f"{colour('PART', YELLOW)}  {outcome.name}  {pct:5.1f}% of reference API")
        else:
            failures += 1
            print(f"{colour('FAIL', RED)}  {outcome.name}")
            for message in outcome.messages:
                print(f"      {message}")

    total = len([o for o in outcomes if o.status != "partial"])
    print(f"\n{total - failures}/{total} cases passed", end="")
    partials = [o for o in outcomes if o.status == "partial"]
    if partials:
        mean = sum(o.coverage or 0.0 for o in partials) / len(partials) * 100
        print(f", {len(partials)} partial (mean coverage {mean:.1f}%)", end="")
    print()
    return 1 if failures else 0


def show(case: Path) -> int:
    """Print all three streams for one case, side by side."""
    runs = [
        ("REF", run_case(case, REF_PYTHON, {})),
        ("COMPAT", run_case(case, OUR_PYTHON, {"LANELET2_BUG_COMPAT": "1"})),
        ("FIXED", run_case(case, OUR_PYTHON, {})),
    ]
    keys: list[str] = []
    for _, run in runs:
        for record in run.records:
            if record.key not in keys:
                keys.append(record.key)

    print(f"=== {case.stem} ===")
    for label, run in runs:
        if run.returncode != 0:
            print(f"{label} exited {run.returncode}")
            if run.stderr.strip():
                print(indent(run.stderr.strip(), "    "))
    for key in keys:
        print(f"\n{key}")
        for label, run in runs:
            value = run.as_dict().get(key, "<absent>")
            print(f"  {label:<7} {render(value, 400)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
