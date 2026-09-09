#!/usr/bin/env python3
"""Run the Python gate this half was ported from, and print what it found as JSON.

The adapter half of `tests/parity.rs`. It exists so that the parity harness compares *findings*
rather than two renderings of findings: the Python entry point prints warnings to stdout and
failures to stderr, and a harness that scraped those would be testing two formatters as much as two
sets of rules.

It also means the chart repository needs no change to be an oracle. Nothing is added there, nothing
is exported, and when the Python is deleted this script stops resolving — which is exactly the
signal the harness needs to fall back to checking the Rust side alone against the same tree.

Usage: python oracle.py <helm-charts root> <rendered dir>
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2

    root = Path(argv[0]).resolve()
    rendered = Path(argv[1]).resolve()
    scripts = root / ".github" / "scripts"

    entry = scripts / "check-config.py"
    if not entry.is_file():
        print(f"{entry}: the Python gate is not there", file=sys.stderr)
        return 3

    # Ahead of the entry point's own insert, so its sibling modules resolve from this tree rather
    # than from whatever else happens to be importable in the environment running the harness.
    sys.path.insert(0, str(scripts))

    spec = importlib.util.spec_from_file_location("check_config", entry)
    if spec is None or spec.loader is None:
        print(f"{entry}: cannot be imported", file=sys.stderr)
        return 3
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    from config_report import Report  # noqa: PLC0415  (only importable once the path is set)

    report = Report()
    try:
        runner = module.Runner(root / "charts", rendered, module.find_jv(), report)
        charts = runner.run()
    except Exception as failure:  # noqa: BLE001  (the harness wants the message, not a traceback)
        print(json.dumps({"error": f"{type(failure).__name__}: {failure}"}))
        return 0

    print(
        json.dumps(
            {
                "charts": charts,
                "findings": [
                    {"where": where, "level": finding.level, "message": finding.message}
                    for where, finding in report.findings
                ],
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
