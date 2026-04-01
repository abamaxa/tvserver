.PHONY: webserver
webserver:
	RUST_LOG=info SQLX_OFFLINE=true DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo build --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features

.PHONY: run-webserver
run-webserver:
	RUST_LOG=info SQLX_OFFLINE=true DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo run --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features

.PHONY: test
test:
	cargo test --no-default-features --features webserver --lib
