#!/usr/bin/env bash
# Single declared source of truth for every changelog the release pipeline
# seeds and checks. `.ci/changelog-seed.sh` and `.ci/changelog-check.sh` both
# source this file for their default file set instead of each hardcoding its
# own path — a newly-added publishable package that ships a CHANGELOG.md
# gets added here, once, so its omission (forgetting to add it) is visible
# by inspection rather than a path quietly missing from one script but not
# the other.
#
# Paths are relative to the repository root. This file only declares data —
# it is meant to be `source`d, not executed directly.

CHANGELOG_FILES_DEFAULT=(
  "CHANGELOG.md"
  "dart-packages/cratestack_cbor/CHANGELOG.md"
  "dart-packages/cratestack_annotations/CHANGELOG.md"
  "dart-packages/cratestack_builder/CHANGELOG.md"
)
