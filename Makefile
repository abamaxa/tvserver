.PHONY: webserver
webserver:
	RUST_LOG=info SQLX_OFFLINE=true DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo build --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features

.PHONY: run-webserver
run-webserver:
	RUST_LOG=info SQLX_OFFLINE=true DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo run --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features

.PHONY: test
test: test_rust test-js
	echo "All tests passed"

.PHONY: test_rust
test_rust:
	DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo test --tests
