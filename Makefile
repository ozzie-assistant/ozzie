.PHONY: build test lint check clean run-gateway run-ask docker

build:
	cargo build --release --package ozzie-cli

test:
	cargo test --release --workspace

lint:
	cargo clippy --release --workspace -- -D warnings

# Quality gates — all three must pass
check:
	cargo check --release --workspace
	cargo clippy --release --workspace -- -D warnings
	cargo test --release --workspace

clean:
	cargo clean

run-gateway:
	OZZIE_PATH=./dev_home cargo run --release --package ozzie-cli -- gateway

run-ask:
	OZZIE_PATH=./dev_home cargo run --release --package ozzie-cli -- ask "Hello, who are you?"

docker:
	cargo build --release --package ozzie-cli --target x86_64-unknown-linux-musl
	docker build --build-arg BINARY=target/x86_64-unknown-linux-musl/release/ozzie -t ozzie:local .
