"""The configuration a small "orders" service reads at boot, and the contract it publishes.

Mirrors `rust/examples/service/config.rs` and `java/terrace-config-example-service` field for
field, under the same `ORDERS_` prefix, so the three tell one story in three languages.

    python3 orders.py > contract.json
"""

import dataclasses
import enum
import sys

from terrace_producer import Dialect, External, contract, setting, table, to_json

DIALECT = Dialect(prefix="ORDERS_")
APP = {"name": "orders-service", "version": "v1.0.0"}


class LogLevel(enum.Enum):
    """How much the service says, from quietest to loudest."""

    TRACE = "trace"
    DEBUG = "debug"
    INFO = "info"
    WARN = "warn"


@dataclasses.dataclass
class Database:
    """The database this service persists orders to."""

    url: str = setting(
        "postgres://localhost/orders_dev",
        "Connection string. A real deployment supplies it through the secrets directory or a "
        "`_FILE` indirection, never through a plain environment variable.",
        secret=True,
    )
    max_connections: int = setting(10, "Maximum number of pooled connections.")


@dataclasses.dataclass
class Config:
    """The root configuration: everything the service needs before it opens a socket."""

    bind_addr: str = setting("127.0.0.1", "Address the HTTP listener binds to.")
    port: int = setting(8080, "Port the HTTP listener binds to.")
    database: Database = table(Database, "Where orders are persisted.")
    log_level: LogLevel = setting(LogLevel.INFO, "How much the service says.")


EXTERNAL = [
    External(
        "SSL_CERT_FILE",
        owner="openssl",
        doc="CA bundle outbound TLS verifies against. Read by OpenSSL, not by this loader.",
    ),
]
IGNORE = ["KUBERNETES_*", "HOSTNAME"]


def document() -> bytes:
    """The contract's bytes, derived from the types above and their default instance."""
    return to_json(
        contract(Config, Config(), app=APP, dialect=DIALECT, external=EXTERNAL, ignore=IGNORE)
    )


if __name__ == "__main__":
    sys.stdout.buffer.write(document())
