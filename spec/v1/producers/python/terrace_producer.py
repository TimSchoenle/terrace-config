"""Derive a configuration contract from dataclass types.

The reference producer for `spec/v1/PRODUCER.md`, written against the standard library alone.
Everything here is the per-language half of the seam: reading the language's own types and
writing one JSON document. Rendering, label blocks, the twelve refusals and every image check
belong to `terrace-contract`, and none of them is reimplemented here.

A configuration is a tree of dataclasses. A leaf field is a key; a field whose type is itself a
dataclass is a table, and contributes its own fields under its name. What the type cannot say is
carried in `dataclasses.field` metadata, written through `setting`, `required` and `table`.
"""

import dataclasses
import enum
import json
import types
import typing
from collections.abc import Callable, Mapping, Sequence
from typing import Any, TypeVar

Json = Any
T = TypeVar("T")

PRODUCER = {
    "name": "terrace-config-python-reference",
    "version": "0.1.0",
    # No loader is linked, so nothing was measured: see the README's "Reads" section.
    "loader": "python-reference",
}


def setting(default: T, doc: str, *, secret: bool = False) -> T:
    """A key with a default. `secret` marks a credential, whose default is never published."""
    return dataclasses.field(default=default, metadata={"doc": doc, "secret": secret})


def required(doc: str, *, secret: bool = False) -> Any:
    """A key some layer must supply: no default, so the dataclass cannot be built without it."""
    return dataclasses.field(metadata={"doc": doc, "secret": secret})


def table(factory: Callable[[], T], doc: str) -> T:
    """A nested table. Its keys are its own fields, under this field's name."""
    return dataclasses.field(default_factory=factory, metadata={"doc": doc})


@dataclasses.dataclass(frozen=True)
class Dialect:
    """How a key path becomes a name in each layer."""

    prefix: str
    nesting_separator: str = "__"
    indirection_suffix: str = "_FILE"

    def name(self, path: str) -> str | None:
        """The path's name in every layer, or None when no name reaches it.

        The environment layer folds a name to lower case on the way in, so a path is nameable
        exactly when upper-casing its name and folding it back returns the path: `maxAgeSecs` does
        not survive, and neither does a path already carrying the separator. The secrets directory
        uses the name as it is; the environment, prefixed and upper-cased.
        """
        if self.nesting_separator in path:
            return None
        name = path.replace(".", self.nesting_separator)
        back = name.upper().lower().replace(self.nesting_separator, ".")
        return name if back == path else None

    def env(self, path: str) -> str | None:
        """The variable naming `path`, or None when no variable can."""
        name = self.name(path)
        return None if name is None else self.prefix + name.upper()


@dataclasses.dataclass(frozen=True)
class External:
    """A variable the image reads that is nobody's configuration key."""

    name: str
    owner: str
    doc: str
    ty: type = str


@dataclasses.dataclass(frozen=True)
class Leaf:
    """What one Python type says about the value a key holds."""

    ty: str
    constraint: dict[str, Json]
    text_form: str
    values: list[str]


def describe(tp: object) -> Leaf:
    """The constraint and text form a leaf type justifies. Anything else is refused."""
    if tp is bool:  # Before int: bool is a subclass of it.
        return Leaf("bool", {"type": "boolean"}, "boolean", [])
    if tp is int:
        return Leaf("int", {"type": "integer"}, "integer", [])
    if tp is str:
        return Leaf("str", {"type": "string"}, "text", [])
    if isinstance(tp, type) and issubclass(tp, enum.Enum):
        values = [member.value for member in tp]
        if not all(isinstance(value, str) for value in values):
            raise TypeError(f"{tp.__name__}: a choice needs string values")
        return Leaf(tp.__name__, {"enum": values, "type": "string"}, "choice", values)
    raise TypeError(f"{tp!r} has no constraint this producer can state")


def strip_optional(tp: object) -> tuple[object, bool]:
    """`X | None` as `(X, True)`; any other type as `(tp, False)`."""
    if typing.get_origin(tp) in (typing.Union, types.UnionType):
        rest = [arg for arg in typing.get_args(tp) if arg is not type(None)]
        if len(rest) == 1:
            return rest[0], True
    return tp, False


def display(value: Json) -> str | None:
    """A default rendered for a person: bare text, `true`, `8080`."""
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def as_json(value: object) -> Json:
    """A default as a JSON value."""
    return value.value if isinstance(value, enum.Enum) else value


def keys(cls: type, defaults: object, dialect: Dialect, at: str = "") -> list[dict[str, Json]]:
    """Every key of `cls`, depth first in declaration order, with defaults read from `defaults`."""
    out: list[dict[str, Json]] = []
    hints = typing.get_type_hints(cls)
    for field in dataclasses.fields(cls):
        path = at + field.name
        tp, optional = strip_optional(hints[field.name])
        doc = field.metadata.get("doc", "")
        if dataclasses.is_dataclass(tp) and isinstance(tp, type):
            out.extend(keys(tp, getattr(defaults, field.name), dialect, path + "."))
            continue
        leaf = describe(tp)
        secret = bool(field.metadata.get("secret", False))
        has_default = (
            field.default is not dataclasses.MISSING
            or field.default_factory is not dataclasses.MISSING
        )
        # A secret's default is a credential in a public registry: published as no default.
        value = as_json(getattr(defaults, field.name)) if has_default and not secret else None
        env = dialect.env(path)
        key: dict[str, Json] = {
            "path": path,
            "env": env,
            "env_file": None if env is None else env + dialect.indirection_suffix,
            "secrets_file": dialect.name(path),
            "docs": doc,
            "ty": leaf.ty,
            "values": leaf.values,
            "constraint": leaf.constraint,
            "text_form": leaf.text_form,
            "aliases": [],
            "env_aliases": [],
            "env_file_aliases": [],
            "secrets_file_aliases": [],
        }
        if env is None:
            key["unreachable"] = "unnameable"
        key |= {
            "default": display(value),
            "default_value": value,
            "note": None,
            "required": not has_default and not optional,
            "secret": secret,
            "reserved": False,
        }
        out.append(key)
    return out


def refuse_indirection(found: Sequence[Mapping[str, Json]]) -> None:
    """Refusal 8: one variable filling two keys. Refused here, since it cannot be published."""
    indirections = {key["env_file"]: key["path"] for key in found if key["env_file"]}
    for key in found:
        if key["env"] in indirections:
            raise ValueError(
                f"{key['path']}: {key['env']} is also {indirections[key['env']]}'s indirection"
            )


def json_schema(found: Sequence[Mapping[str, Json]], title: str) -> dict[str, Json]:
    """The keys as a closed draft-07 schema, nested at each key's position in the document."""
    root: dict[str, Json] = {"additionalProperties": False, "properties": {}, "type": "object"}
    for key in found:
        node = root
        *tables, name = key["path"].split(".")
        for segment in tables:
            node = node["properties"].setdefault(
                segment, {"additionalProperties": False, "properties": {}, "type": "object"}
            )
        prop = dict(key["constraint"])
        if key["docs"]:
            prop["description"] = key["docs"]
        if key["default_value"] is not None:
            prop["default"] = key["default_value"]
        if key["secret"]:
            prop["writeOnly"] = True
        node["properties"][name] = prop
    return {"$schema": "http://json-schema.org/draft-07/schema#", **root, "title": title}


def external_var(var: External) -> dict[str, Json]:
    """One declared external variable, with the two-space fields a key has."""
    leaf = describe(var.ty)
    return {
        "name": var.name,
        "owner": var.owner,
        "docs": var.doc,
        "ty": leaf.ty,
        "values": leaf.values,
        "constraint": leaf.constraint,
        "text_form": leaf.text_form,
        "required": False,
        "secret": False,
    }


def contract(
    cls: type,
    defaults: object,
    *,
    app: Mapping[str, str],
    dialect: Dialect,
    external: Sequence[External] = (),
    ignore: Sequence[str] = (),
) -> dict[str, Json]:
    """The whole document for one configuration root."""
    found = keys(cls, defaults, dialect)
    refuse_indirection(found)
    title = f"{app['name']} configuration"
    return {
        "terrace_contract": 1,
        "producer": PRODUCER,
        "app": dict(app),
        "schema": {
            "schema_version": 2,
            "dialect": dataclasses.asdict(dialect),
            "loader": [
                {
                    "env": dialect.prefix + "CONFIG",
                    "role": "config",
                    "docs": "Names the TOML document layer.",
                    "default": "config.toml",
                },
                {
                    "env": dialect.prefix + "SECRETS_DIR",
                    "role": "secrets_dir",
                    "docs": "Names a directory of key-named files. Each file supplies the key "
                    "its name spells.",
                    "default": None,
                },
            ],
            "keys": found,
        },
        "json_schema": sort(json_schema(found, title)),
        "external": {
            "env": [external_var(var) for var in external],
            "ignore": list(ignore),
            "unknown": "reject",
        },
    }


def sort(value: Json) -> Json:
    """Every object's members in name order, at every depth."""
    if isinstance(value, dict):
        return {name: sort(value[name]) for name in sorted(value)}
    if isinstance(value, list):
        return [sort(item) for item in value]
    return value


def to_json(document: Mapping[str, Json]) -> bytes:
    """The document's bytes: two-space indent, UTF-8 unescaped, one trailing newline.

    Byte-stable by construction. Every member is written in an order fixed by this source
    (insertion order for the envelope and each key, name order inside `json_schema`), and nothing
    in it is read from the clock, the environment or the filesystem.
    """
    return (json.dumps(document, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
