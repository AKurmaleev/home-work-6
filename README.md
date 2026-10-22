# Homework 6 — Thread Pool with Mutex and Condvar

Solution to the thread synchronization assignment. The public API and package name from the template are preserved: `ThreadPool::new`, `execute`, `shutdown`, `Task = fn(i64)`.

## Reviewer navigation

- [src/lib.rs](src/lib.rs) — pool implementation.
- [REPORT.md](REPORT.md) — invariants, shutdown, and verification results.
- [tests/thread_pool.rs](tests/thread_pool.rs) — 9 original tests; disabled when Loom is enabled because they use regular threads outside the model environment.
- [tests/lifecycle.rs](tests/lifecycle.rs) — 3 additional checks for concurrency, FIFO, and worker panics.
- [tests/loom_thread_pool.rs](tests/loom_thread_pool.rs) — model checks of thread interleavings.
- [ASSIGNMENT.md](ASSIGNMENT.md) — original assignment.

## Verification

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --features loom --test loom_thread_pool --release
```

The package is a library; `cargo run` is not needed. The `loom` feature is intended only for model tests, not for normal library use. The first Loom run may download dependencies from `Cargo.lock`.

## Implementation

Workers share an `Arc<Shared>`. The `VecDeque<i64>` queue and shutdown flag are protected by the same `Mutex`; an empty queue causes workers to sleep on a `Condvar` rather than busy-wait. The task is called after the guard is released. `shutdown(self)` wakes everyone, allows the remaining queue to drain, and joins each worker.

The FIFO queue determines dequeue order, but multiple workers do not guarantee callback start or completion order. This educational API is intended for a finite workload: the queue is unbounded, there is no timeout/cancellation, and an explicit `shutdown` is required to stop the pool. The guarantee that all accepted work is processed assumes that tasks finish without panicking. On a panic, shutdown joins the workers and reports the panic; preservation of the remaining work is not promised.
