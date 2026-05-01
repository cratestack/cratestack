set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
  @just --list


all-checks:
	@echo "Running Rust formatting, lint, and checks"
	cargo fmt
	cargo fix --allow-dirty
	cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings
	cargo check --all-targets --all-features
	cargo deny check
