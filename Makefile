.PHONY: test test-all

test: test-all

test-all:
	cargo test
	cargo test --features loom --test loom_thread_pool
