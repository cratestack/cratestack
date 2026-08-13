# Builds a `wiremock/wiremock` image with `wiremock-state-extension`
# correctly loaded, for the STATEFUL `transport rest` model CRUD stubs
# `cratestack generate-wiremock` emits (`transport rpc` and every
# procedure stub stay static and need nothing beyond this — see
# ../README.md).
#
# This is a build, not a `docker run wiremock/wiremock -v
# extensions:/var/wiremock/extensions` — that does NOT work. The
# extension's plain Maven Central jar throws `AbstractMethodError`/
# `NoSuchMethodError` against every `wiremock/wiremock` image tested
# (3.7.0, 3.9.1, 3.13.2 — the extension's own upstream issue #36
# confirms this is a known, real packaging defect: its Handlebars
# `Helper` classes are compiled against an unrelocated
# `com.github.jknack.handlebars`, but every WireMock standalone
# distribution relocates that package internally). The extension's own
# `shadowJar` Gradle task relocates it correctly and DOES work — but
# that artifact is never published anywhere (only built as an ephemeral
# CI artifact upstream), so building it from pinned source is the only
# reproducible option today. See `docs/design/wiremock-stubs.md`'s
# "Model CRUD statefulness" section for the full investigation and
# evidence this conclusion is based on.
#
# Pinned, deliberately not "latest" (cratestack's own Trunk/wasm-bindgen
# incidents are why):
#   - wiremock-state-extension @ 0d9fff0554319bc5e62310137a6b225a9760e002
#     (tag 0.10.1) — https://github.com/wiremock/wiremock-state-extension
#   - wiremock/wiremock:3.13.2 — the WireMock version that exact commit's
#     own `build.gradle` (`versions.wiremock`) targets; also the version
#     this generator's stubs were verified against by hand.
# Bump both together, from the extension's `build.gradle` at whatever
# commit you're moving to — mismatched versions are exactly the
# `AbstractMethodError` class of failure this Dockerfile exists to avoid.

FROM eclipse-temurin:21-jdk AS extension-build

ARG WIREMOCK_STATE_EXTENSION_COMMIT=0d9fff0554319bc5e62310137a6b225a9760e002

WORKDIR /build
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --quiet https://github.com/wiremock/wiremock-state-extension.git . \
 && git checkout --quiet "${WIREMOCK_STATE_EXTENSION_COMMIT}"
RUN ./gradlew --no-daemon shadowJar

FROM wiremock/wiremock:3.13.2
COPY --from=extension-build /build/build/libs/wiremock-state-extension-standalone-*.jar /var/wiremock/extensions/

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
