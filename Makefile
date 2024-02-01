
.PHONY: test
test: test_rust test-js
	echo "All tests passed"

 .PHONY: build
build:
	@cd client_src/remote_player && \
    npm run export && \
    cd ../tvremote && \
    npm run export

.PHONY: test_rust
test_rust:
	DATABASE_URL="sqlite:memory:" DATABASE_MIGRATION_DIR="migrations" cargo test --tests

 .PHONY: test-js
test-js:
	@cd client_src/remote_player && \
    npm run test -- --watchAll=false && \
    cd ../tvremote && \
    npm run test

.PHONY: install
install:
	@cd client_src/remote_player && \
    npm i && \
    cd ../tvremote && \
    npm i