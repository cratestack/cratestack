# Builds a `wiremock/wiremock` image with `wiremock-state-extension`
# correctly loaded, for the STATEFUL `transport rest` model CRUD stubs
# `cratestack generate-wiremock` emits (`transport rpc` and every
# procedure stub stay static and need nothing beyond this — see
# ../README.md).
#
# This is a build, not a `docker run wiremock/wiremock -v
# extensions:/var/wiremock/extensions` — mounting the extension's plain
# (non-`-standalone`) Maven Central jar does NOT work. That artifact
# throws `AbstractMethodError`/`NoSuchMethodError` at request time
# against every `wiremock/wiremock` image this repo tested by hand
# (3.7.0, 3.9.1, 3.13.2 — see `docs/design/wiremock-stubs.md`'s "Model
# CRUD statefulness" section for the full investigation). Root cause:
# every WireMock standalone distribution relocates its bundled
# Handlebars (`com.github.jknack.handlebars` →
# `wiremock.com.github.jknack.handlebars`); the plain jar's `Helper`
# classes are compiled against the unrelocated package, so they don't
# match the ABI the relocated runtime expects.
#
# Note on the upstream evidence, because an earlier version of this
# comment overstated it: the extension's issue #36 is the identical
# `AbstractMethodError`, but it was CLOSED AS COMPLETED on 2023-08-24
# ("the package relocation was wrong"). A 2024-11-22 comment reports a
# recurrence and the maintainer asked for a fresh issue, which was
# apparently never filed. So #36 is corroborating context for the
# failure mode, not an open upstream defect — the actual evidence that
# the plain jar does not work against these images is this repo's own
# empirical testing recorded above.
#
# What does work is the `-standalone` classifier artifact, which IS the
# output of the extension's own `shadowJar` task (Handlebars relocated
# the same way WireMock's distribution does) and IS published to Maven
# Central. Verified on the exact jar pinned below: 572 entries under
# `wiremock/com/github/jknack`, 0 under an unrelocated
# `com/github/jknack`. An earlier version of this file built that jar
# from a git clone + `./gradlew shadowJar` because the comment claimed
# the shaded artifact "is never published anywhere" — that was wrong
# from release 0.9.x on. (Easy to get wrong: `search.maven.org`'s index
# is stale and still reports 0.8.2 as the newest `-standalone`; only the
# repo1 directory listing shows 0.9.3 … 0.10.1.)
#
# Pinned, deliberately not "latest" (cratestack's own Trunk/wasm-bindgen
# incidents are why), and verified by sha256 at build time so a
# republished or MITM'd URL fails the build rather than silently
# changing the image:
#   - org.wiremock.extensions:wiremock-state-extension-standalone:0.10.1
#     from Maven Central (repo1), sha256
#     8898a2700ca16f2d235fa42253e3f400cc08117f9190fdfe1686014c31619ae7
#     (2682223 bytes) — https://github.com/wiremock/wiremock-state-extension
#   - wiremock/wiremock:3.13.2 — the WireMock version tag 0.10.1's own
#     `build.gradle:18` targets (`versions.wiremock = '3.13.2'`); also
#     the version this generator's stubs were verified against by hand.
# This pair is load-bearing: bump both together, from the extension
# release's `build.gradle`, and re-derive the sha256. Mismatched
# versions are exactly the `AbstractMethodError` class of failure this
# Dockerfile exists to avoid. As of this writing 3.13.2 + 0.10.1 is
# already the newest matched pair that exists — WireMock 4.x is
# beta-only with no extension release targeting it.

FROM wiremock/wiremock:3.13.2

ARG WIREMOCK_STATE_EXTENSION_VERSION=0.10.1
ARG WIREMOCK_STATE_EXTENSION_SHA256=8898a2700ca16f2d235fa42253e3f400cc08117f9190fdfe1686014c31619ae7

# `curl` and `sha256sum` are both already present in the base image
# (Ubuntu 26.04 + coreutils), so this needs no extra fetch stage and no
# package install. `sha256sum -c` exits non-zero on a mismatch, which
# under this `RUN` fails the build.
RUN set -eu; \
    mkdir -p /var/wiremock/extensions; \
    jar="/var/wiremock/extensions/wiremock-state-extension-standalone-${WIREMOCK_STATE_EXTENSION_VERSION}.jar"; \
    curl --fail --silent --show-error --location --retry 3 --output "${jar}" \
      "https://repo1.maven.org/maven2/org/wiremock/extensions/wiremock-state-extension-standalone/${WIREMOCK_STATE_EXTENSION_VERSION}/wiremock-state-extension-standalone-${WIREMOCK_STATE_EXTENSION_VERSION}.jar"; \
    echo "${WIREMOCK_STATE_EXTENSION_SHA256}  ${jar}" | sha256sum -c -

# Run as non-root. `wiremock/wiremock:3.13.2` runs as root out of the box,
# which trips this repo's own Dockerfile scan (Trivy DS-0002, HIGH) in the
# `quality` CI job — and is worth fixing on its merits, not just to get a
# green check: this image exists to be run by consumers and in examples,
# and nothing it does needs root.
#
# `ubuntu` (uid 1000) already exists in the base image, so no `useradd`
# layer is needed. WireMock listens on 8080, above the privileged-port
# threshold, and the directories it reads (`/var/wiremock/extensions`,
# `/home/wiremock/*` mappings mounted by the caller) are world-readable —
# but its default `--root-dir` is `/home/wiremock`, which it writes to for
# request journalling and recorded mappings, so that has to be owned by
# the runtime user or startup fails on a read-only path.
RUN chown -R 1000:1000 /home/wiremock /var/wiremock
USER 1000:1000
# `--global-response-templating` is the default here so the image works
# with no extra flags — every generated stub also declares
# `"transformers": ["response-template"]` itself, so this is redundant
# but harmless if a consumer already passes it explicitly.
CMD ["--global-response-templating"]
