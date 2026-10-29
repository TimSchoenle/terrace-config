#!/usr/bin/env bash
#
# Emits the one README fact readme-variables cannot derive, as strict JSON on stdout: the release
# series an image tag pins.
#
# It is the `extra` input of the `readme-variables` step that renders cli/README.md in docs.yml,
# deep-merged over what that action derives. Its only key, `image`, collides with nothing the
# action emits (`repo`, `release`, `toolchain` and `docs`).
#
# The release workflow tags the image with the full version, `major.minor` and `major`. Below 1.0 a
# minor release may break, so the tag a README tells a build to pin is `0.minor`; from 1.0 it is the
# major alone. Handlebars has no helper that splits a version, so the series is computed here.
#
# Run it yourself to see what CI will render with:
#
#     bash .github/scripts/readme-variables.sh cli/Cargo.toml
#
# POSIX tools only, no `jq`: it is not present in a default Git for Windows shell, and a script
# that only runs on the CI runner is a script nobody checks their edit against.
set -euo pipefail

manifest="${1:?usage: readme-variables.sh <Cargo.toml>}"

# The first line that starts `version = "..."` is the `[package]` version: a dependency's version
# sits inside an inline table and never starts a line.
version="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "${manifest}" | head -n1)"

if [ -z "${version}" ]; then
    echo "readme-variables: no top-level 'version' in ${manifest}" >&2
    exit 1
fi

# Constraining the alphabet to digits and dots is what makes the `printf` below safe without a
# JSON encoder.
if ! printf '%s' "${version}" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "readme-variables: 'version = \"${version}\"' is not a release version" >&2
    exit 1
fi

major="${version%%.*}"
rest="${version#*.}"
minor="${rest%%.*}"

if [ "${major}" = "0" ]; then
    series="0.${minor}"
else
    series="${major}"
fi

printf '{"image":{"series":"%s"}}\n' "${series}"
