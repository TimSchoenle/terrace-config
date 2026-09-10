# The container image §9.1 of `docs/contract-cli-plan.md` calls the channel that matters most.
#
# It exists so that a build integration is language-agnostic: a build stage needs no toolchain, no
# download step and no platform detection, only a `COPY --from`.
#
#     FROM ghcr.io/timschoenle/terrace-contract:1 AS contract
#     COPY --from=contract-builder /out/contract.json /in/contract.json
#     RUN terrace-contract conform --tier 2 /in/contract.json \
#      && terrace-contract render --format labels /in/contract.json > /out/labels
#
# Every release publishes three tag precisions — `1`, `0.2` and `0.2.2` — and a Dockerfile pins
# the one it wants: the major to take every fix without an edit, the exact version to take none.
#
# Built from binaries the release workflow already cross-compiled and signed, rather than compiling
# inside the image. Two reasons, and the second is the load-bearing one: a `cargo build` per
# architecture under emulation costs an hour where a `COPY` costs a second, and the bytes in the
# image are then *the same bytes* the release attached and cosign signed — where a second build
# would produce a second artefact nobody verified against the first.
#
# `scratch` rather than a distribution base. The binary is statically linked against musl and reads
# no configuration of its own; what a base image would add is a package manager, a shell and a CVE
# feed for a container whose whole job is to run one command and exit. The certificate bundle is not
# needed either: nothing here opens a socket — `pull` shells out to `oras` and `cosign`, which is
# §9.5's delegation, and those are not in this image.
FROM scratch

# The per-architecture binary buildx selected, staged by the workflow under the path buildx's own
# `TARGETPLATFORM` names. One `COPY` for every architecture, resolved at build time.
ARG TARGETARCH
COPY dist/${TARGETARCH}/terrace-contract /terrace-contract

# Not `root`. Nothing here writes, reads a secret or binds a port, so there is nothing the number
# needs to own — which is exactly why it costs nothing to be explicit, and why a reader should not
# have to check.
USER 65532:65532

ENTRYPOINT ["/terrace-contract"]
