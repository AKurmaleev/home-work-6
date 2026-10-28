# Homework 6 — Thread Pool with Mutex and Condvar

Solution to the thread synchronization assignment. The public API and package name from the template are preserved: `ThreadPool::new`, `execute`, `shutdown`, `Task = fn(i64)`.

## Reviewer navigation

- [src/lib.rs](src/lib.rs) — pool implementation.
- [REPORT.md](REPORT.md) — invariants, shutdown, and verification results.
- [tests/thread_pool.rs](tests/thread_pool.rs) — 9 original tests; disabled when Loom is enabled because they use regular threads outside the model environment.
- [tests/lifecycle.rs](tests/lifecycle.rs) — 4 checks for concurrency, FIFO, single-worker panic propagation, and multiple panic-payload ownership.
- `src/lib.rs` unit tests — 2 injected startup-failure checks, including combined startup/join failures.
- [tests/loom_thread_pool.rs](tests/loom_thread_pool.rs) — model checks of thread interleavings.
- [ASSIGNMENT.md](ASSIGNMENT.md) — original assignment.

## Verification

```sh
cargo fmt --all --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings -D unused_must_use -D clippy::let_underscore_must_use -D clippy::unused_result_ok
cargo test --locked
cargo test --locked --features loom --test loom_thread_pool --release -- --test-threads=1
```

`make test` / `make test-all` runs these same gates. MSRV is Rust 1.85 (edition 2024).

The package is a library; `cargo run` is not needed. The `loom` feature is intended only for model tests, not for normal library use. The first Loom run may download dependencies from `Cargo.lock`.

## Implementation

Workers share an `Arc<Shared>`. The `VecDeque<i64>` queue and shutdown flag are protected by the same `Mutex`; an empty queue causes workers to sleep on a `Condvar` rather than busy-wait. The task is called after the guard is released. `shutdown(self)` wakes everyone, allows the remaining queue to drain, and joins each worker.

The FIFO queue determines dequeue order, but multiple workers do not guarantee callback start or completion order. This educational API is intended for a finite workload: the queue is unbounded, there is no timeout/cancellation, and an explicit `shutdown` is required to stop the pool. The guarantee that all accepted work is processed assumes that tasks finish without panicking. On a panic, shutdown joins the workers and reports the panic; preservation of the remaining work is not promised. A single worker panic retains its original payload. Multiple worker panics are transferred to the caller as `Vec<Box<dyn std::any::Any + Send>>` in join order, without destroying secondary payloads during unwinding. Catching callers own payload disposal; destructors that panic still require care. This policy applies to unwinding panics, not process aborts.

If worker creation fails, the constructor stops and joins all previously started workers before panicking. If those joins also report panics, the vector payload contains the original `std::io::Error` first, followed by all worker panic payloads. No tasks have been accepted during construction. Mutex poisoning is rejected rather than silently recovered.

Loom 0.7.2 checks six small normal-operation scenarios using the real pool algorithm with instrumented synchronization. The two-worker/two-task model uses `preemption_bound = Some(2)`; the other five use default exploration. No permutation or duration cap is set in the tests. External command deadlines bound review runs; these models do not cover startup failure, task panics, OS scheduling, poisoning, aborts, or arbitrary workloads.
