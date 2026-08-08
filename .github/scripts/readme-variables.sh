#!/usr/bin/env bash
#
# Emits the variable payload for `.github/templates/README.md.hbs` as strict JSON on stdout.
#
# `Cargo.toml` is the single source of truth for both numbers the README quotes: the tag its
# install snippet pins, and the MSRV its badge advertises. Deriving them here rather than
# hand-editing the README is what lets the release pull request — the commit that bumps
# `version` — carry the matching documentation with it.
#
# Run it yourself to see what CI will render with:
#
#     bash .github/scripts/readme-variables.sh
#
# Deliberately POSIX tools only, no `jq`: it is not present in a default Git for Windows shell,
# and a script that only runs on the CI runner is a script nobody checks their edit against.
set -euo pipefail

manifest="${1:-Cargo.toml}"

# Reads a top-level `key = "value"` from the manifest and rejects anything that would need JSON
# escaping. Both fields are version strings, so the accepted alphabet is the whole contract —
# and constraining it is what makes the `printf` at the bottom safe without a JSON encoder.
#
# Only `[package]` keys can match: a dependency's version sits inside an inline table
# (`figment = { version = "0.10", … }`) and never starts a line, so anchoring is enough.
field() {
    local key="$1" pattern="$2" value
    value="$(sed -n "s/^${key} = \"\([^\"]*\)\".*/\1/p" "${manifest}" | head -n1)"

    if [ -z "${value}" ]; then
        echo "readme-variables: no top-level '${key}' in ${manifest}" >&2
        return 1
    fi

    if ! printf '%s' "${value}" | grep -Eq "${pattern}"; then
        echo "readme-variables: '${key} = \"${value}\"' is not a version string" >&2
        return 1
    fi

    printf '%s' "${value}"
}

version="$(field version '^[0-9A-Za-z][0-9A-Za-z.+-]*$')"
msrv="$(field rust-version '^[0-9]+(\.[0-9]+){0,2}$')"

printf '{"version":"%s","tag":"v%s","msrv":"%s"}\n' "${version}" "${version}" "${msrv}"
