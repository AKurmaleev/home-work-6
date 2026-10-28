.PHONY: test test-all

test: test-all

test-all:
	cargo fmt --all --check
	cargo check --locked --all-targets
	cargo clippy --locked --all-targets -- -D warnings -D unused_must_use -D clippy::let_underscore_must_use -D clippy::unused_result_ok
	cargo test --locked
	cargo test --locked --features loom --test loom_thread_pool --release -- --test-threads=1
