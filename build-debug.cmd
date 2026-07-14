@echo off

call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvarsall.bat" arm64

set CC=
set RUST_LOG=info
set SQLX_OFFLINE=true
set DATABASE_URL="sqlite:memory:"
set DATABASE_MIGRATION_DIR="migrations" 

cargo build --bin=tvserver --package=tvserver --manifest-path=./Cargo.toml --features=webserver --no-default-features
