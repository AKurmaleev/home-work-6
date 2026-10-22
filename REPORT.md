# Brief report on Homework 6

## Implementation

A fixed-size thread pool replaces the stubs in `src/lib.rs`. Each task is an `i64` argument for a single `fn(i64)` function. The FIFO queue (`VecDeque`) does not require shifting elements when removing from the front. `Arc`, `Mutex`, `Condvar`, and `thread` are imported through the provided `sync` module, so the same algorithm is checked with regular primitives and Loom replacements.

## Invariants

1. The queue and `shutting_down` are read and modified under the same mutex.
2. A worker sleeps only while `queue.is_empty() && !shutting_down`. `wait` atomically releases the guard when entering the wait and returns it after waking. The condition is rechecked in a `while` loop: waking does not guarantee that work is available.
3. Dequeuing an argument is atomic with respect to other workers. For tasks that finish normally, one accepted argument causes one callback; identical values remain separate tasks.
4. The callback runs without the queue guard; otherwise a long-running handler would block producers and all other workers.
5. After a shutdown request, a worker first drains the available queue. An empty queue together with the shutdown flag means exit. Waiting in `join` happens without the queue mutex.

`execute(&self)` supports multiple producers. `shutdown(self)` consumes the pool: safe Rust prevents calling it while another thread is still using a borrowed `&ThreadPool`. Shutdown wakes everyone through `notify_all`; a single `notify_one` would leave the others asleep.

## Verification

Local environment: Windows, Rust/Cargo 1.95.0. Before implementation, 8 original tests failed at `todo!`; the zero-worker test passed only because any panic from the stub satisfied it.

After implementation, `cargo fmt --all --check`, `cargo check --all-targets`, Clippy with `-D warnings` (in normal mode and with `--all-features`), 12 regular tests, and 6 Loom tests passed. Regular and model tests were checked in debug and release.

Coverage:

- 9 original scenarios: empty/working pool, wakeups, draining, concurrent producers, exactly-once execution, and negative arguments;
- 3 additional scenarios: two concurrently running callbacks, duplicate/extreme `i64` values, and panic handling;
- Loom: waking one/two workers, submitting work, producer completion, exact execution of arguments, and competing workers.

The model with two workers and two tasks is limited to two preemptive context switches (`preemption_bound = Some(2)`); the other models use Loom's default exploration. Model checking covers the specified small scenarios, not all possible workloads or OS failures.

Reproduction commands are in [README.md](README.md). The tests make no speed assertions; no statistical benchmark was conducted for this homework.

## Limitations

This is an educational pool, not a production executor. The unbounded queue requires a finite workload; a stuck callback delays shutdown. Tasks are not restarted after a panic. An explicit `shutdown` is required: `Drop` is not implemented. An OS error during thread creation may cause the constructor to panic; cleanup of a partially created pool is not guaranteed by this API.
