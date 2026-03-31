.PHONY: fmt lint check test build coverage coverage-ci coverage-all

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets -- -D warnings

check:
	cargo check --all-targets

test:
	cargo test

build:
	cargo build --release

coverage:
	cargo tarpaulin --out Html

coverage-ci:
	cargo tarpaulin --out Xml --out Lcov

coverage-all:
	cargo tarpaulin --out Html --out Xml --out Lcov
