#!/usr/bin/env python3
"""Run the Python gates this half was ported from, and print what they found as JSON.

The adapter half of `tests/parity.rs`. It exists so that the parity harness compares *findings*
rather than two renderings of findings: the Python entry points print warnings to stdout and
failures to stderr, and a harness that scraped those would be testing two formatters as much as two
sets of rules.

It also means the chart repository needs no change to be an oracle. Nothing is added there, nothing
is exported, and when a gate's Python is deleted its entry point stops resolving — which is exactly
the signal the harness needs to fall back to checking the Rust side alone against the same tree.

Usage: python oracle.py check    <helm-charts root> <rendered dir>
       python oracle.py bindings <helm-charts root> [<charts dir>]
       python oracle.py diff     <helm-charts root> <revision>
       python oracle.py secrets  <helm-charts root> <rendered dir>
       python oracle.py explain  <helm-charts root> <chart> [pattern]

The root always names the checkout the *scripts* come from. A charts directory may be given
separately, so the harness can point the oracle at a mutated copy of the tree without copying the
scripts beside it.
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path


def load(scripts: Path, name: str, file_name: str):
    """One hyphenated entry point, which an `import` statement cannot spell."""
    entry = scripts / file_name
    if not entry.is_file():
        return None
    spec = importlib.util.spec_from_file_location(name, entry)
    if spec is None or spec.loader is None:
        return None
    module = importlib.util.module_from_spec(spec)
    # Registered before it is executed: `dataclasses` resolves a field's annotation by looking the
    # defining module up here, and a module that is not registered fails to define its first frozen
    # dataclass.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def findings(report) -> list[dict]:
    return [
        {"where": where, "level": finding.level, "message": finding.message}
        for where, finding in report.findings
    ]


def main(argv: list[str]) -> int:
    # A document's prose is UTF-8 — an em dash, a non-breaking space — and the default console
    # encoding on Windows is not, so without this the oracle hands the harness mangled bytes and
    # every difference it reports is its own.
    sys.stdout.reconfigure(encoding="utf-8")

    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2

    gate = argv[0]
    root = Path(argv[1]).resolve()
    scripts = root / ".github" / "scripts"

    # Ahead of an entry point's own insert, so its sibling modules resolve from this tree rather
    # than from whatever else happens to be importable in the environment running the harness.
    sys.path.insert(0, str(scripts))

    try:
        if gate == "check":
            module = load(scripts, "check_config", "check-config.py")
            if module is None:
                print(f"{scripts}: the Python gate is not there", file=sys.stderr)
                return 3
            from config_report import Report  # noqa: PLC0415

            report = Report()
            runner = module.Runner(root / "charts", Path(argv[2]).resolve(), module.find_jv(), report)
            charts = runner.run()
            print(json.dumps({"charts": charts, "findings": findings(report)}, indent=2))
            return 0

        if gate == "diff":
            module = load(scripts, "contract_diff_entry", "contract-diff.py")
            if module is None:
                print(f"{scripts}: the Python gate is not there", file=sys.stderr)
                return 3
            import os

            # The entry point resolves paths against the working directory, as a recipe does.
            os.chdir(root)
            from config_report import Report  # noqa: PLC0415

            report = Report()
            revision = module.Revision(argv[2])
            diffs = module.collect(Path("charts"), revision, None, report)
            print(json.dumps(module.as_json(diffs, argv[2], revision.commit, report), indent=2))
            return 0

        if gate == "secrets":
            module = load(scripts, "config_secrets_entry", "config-secrets.py")
            if module is None:
                print(f"{scripts}: the Python gate is not there", file=sys.stderr)
                return 3

            surface = module.reconcile(root / "charts", Path(argv[2]).resolve())
            print(
                json.dumps(
                    {
                        "inventory": module.inventory_json(root / "charts"),
                        "surface": module.surface_json(surface),
                        "findings": findings(module.report_of(surface)),
                    },
                    indent=2,
                )
            )
            return 0

        if gate == "explain":
            module = load(scripts, "explain_config_entry", "explain-config.py")
            if module is None:
                print(f"{scripts}: the Python gate is not there", file=sys.stderr)
                return 3
            from config_declaration import load_declaration  # noqa: PLC0415
            from config_report import Report  # noqa: PLC0415

            chart_dir = root / "charts" / argv[2]
            pattern = argv[3] if len(argv) > 3 else None
            report = Report()
            surface = module.collect(chart_dir, load_declaration(chart_dir), report)
            if surface is None:
                print(json.dumps({"refused": True, "findings": findings(report)}, indent=2))
                return 0
            module.report_divergences(surface, pattern, report)
            print(
                json.dumps(
                    {
                        "surface": module.as_json(surface, pattern),
                        "findings": findings(report),
                    },
                    indent=2,
                )
            )
            return 0

        if gate == "bindings":
            module = load(scripts, "check_config_bindings", "check-config-bindings.py")
            if module is None:
                print(f"{scripts}: the Python gate is not there", file=sys.stderr)
                return 3
            from config_report import Report  # noqa: PLC0415

            report = Report()
            charts = Path(argv[2]).resolve() if len(argv) > 2 else root / "charts"
            enrolled = module.run(charts, report)
            print(
                json.dumps(
                    {
                        "enrolled": [list(row) for row in enrolled],
                        "findings": findings(report),
                    },
                    indent=2,
                )
            )
            return 0
    except Exception as failure:  # noqa: BLE001  (the harness wants the message, not a traceback)
        print(json.dumps({"error": f"{type(failure).__name__}: {failure}"}))
        return 0

    print(f"unknown gate {gate!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
