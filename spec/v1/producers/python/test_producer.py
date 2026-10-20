"""The reference producer's checks: the committed document, and the tier 2 spellings.

python3 -m unittest discover -s spec/v1/producers/python
"""

import dataclasses
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path
from typing import Any

import orders
from terrace_producer import Dialect, contract, required, setting

HERE = Path(__file__).resolve().parent
CORPUS = HERE.parents[1] / "conformance"

# Every field tier 2 holds an implementation to. CONFORMANCE.md, "Tier 2 - dialect".
SPELLINGS = (
    "path",
    "env",
    "env_file",
    "secrets_file",
    "env_aliases",
    "env_file_aliases",
    "secrets_file_aliases",
    "unreachable",
)


class CommittedDocument(unittest.TestCase):
    def test_the_committed_document_is_what_the_types_derive(self) -> None:
        self.assertEqual(
            orders.document(),
            (HERE / "contract.json").read_bytes(),
            "contract.json is stale: run `python3 orders.py > contract.json` and read the diff",
        )

    def test_the_bytes_do_not_depend_on_the_hash_seed(self) -> None:
        # The one input a Python process varies between runs on its own. A document whose bytes
        # moved with it would be byte-stable on one machine and not across two builds.
        for seed in ("0", "1", "4242"):
            env = dict(os.environ, PYTHONHASHSEED=seed, PYTHONDONTWRITEBYTECODE="1")
            out = subprocess.run(
                [sys.executable, str(HERE / "orders.py")],
                env=env,
                check=True,
                capture_output=True,
            ).stdout
            self.assertEqual(out, (HERE / "contract.json").read_bytes(), f"seed {seed}")


@dataclasses.dataclass
class Minimal:
    dist_dir: str = setting("public", "Bundle directory the readiness probe checks.")


@dataclasses.dataclass
class Unnameable:
    distDir: str = setting("public", "Where the bundle is read from.")  # noqa: N815
    maxAgeSecs: int = setting(0, "How long a rendered page stays fresh.")  # noqa: N815


class Tier2AgainstTheCorpus(unittest.TestCase):
    """The corpus cases this producer's type vocabulary can express, spelling by spelling."""

    def assert_spellings(self, case: str, cls: type, prefix: str) -> None:
        expected = json.loads((CORPUS / case / "contract.json").read_text(encoding="utf-8"))
        ours = contract(cls, cls(), app=expected["app"], dialect=Dialect(prefix=prefix))
        self.assertEqual(ours["schema"]["dialect"], expected["schema"]["dialect"])
        self.assertEqual(spellings(ours), spellings(expected))

    def test_minimal(self) -> None:
        self.assert_spellings("minimal", Minimal, "MINIMAL_")

    def test_unnameable_key(self) -> None:
        self.assert_spellings("unnameable-key", Unnameable, "EDGE_")


class Requiredness(unittest.TestCase):
    def test_a_key_without_a_default_is_required_and_an_optional_one_is_not(self) -> None:
        @dataclasses.dataclass
        class Service:
            api_base: str = required("Base URL every outbound request is resolved against.")
            token: str | None = setting(None, "Bearer token.", secret=True)

        document = contract(
            Service, Service(api_base="unused"), app={"name": "s"}, dialect=orders.DIALECT
        )
        api_base, token = document["schema"]["keys"]
        self.assertEqual((api_base["required"], api_base["default_value"]), (True, None))
        self.assertEqual((token["required"], token["ty"], token["secret"]), (False, "str", True))


class Refusals(unittest.TestCase):
    def test_a_key_spelled_as_another_keys_indirection_is_refused(self) -> None:
        @dataclasses.dataclass
        class Clash:
            token: str = setting("", "A token.")
            token_file: str = setting("", "A path.")

        with self.assertRaisesRegex(ValueError, "token_file: ORDERS_TOKEN_FILE"):
            contract(Clash, Clash(), app={"name": "clash"}, dialect=orders.DIALECT)


def spellings(document: dict[str, Any]) -> list[dict[str, Any]]:
    return [{field: key.get(field) for field in SPELLINGS} for key in document["schema"]["keys"]]


if __name__ == "__main__":
    unittest.main()
