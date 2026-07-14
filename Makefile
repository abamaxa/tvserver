.PHONY: webserver
webserver:
	RUST_LOG=info SQLX_OFFLINE=true DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo build --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features

.PHONY: build
build: webserver

.PHONY: run-webserver
run-webserver:
	RUST_LOG=info SQLX_OFFLINE=true DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo run --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features

.PHONY: test-unit
test-unit:
	cargo test --no-default-features --features webserver --lib

.PHONY: test-integration
test-integration:
	set -e; for test in tests/*_test.rs; do \
		cargo test --no-default-features --features webserver --test "$$(basename "$$test" .rs)"; \
	done

.PHONY: test
test: test-unit test-integration
