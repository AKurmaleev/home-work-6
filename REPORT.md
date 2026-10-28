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

## Lifecycle review invariants

The follow-up review keeps the assignment's explicit shutdown API and queue algorithm. The intended ownership and failure contracts are:

- The constructor owns every successfully created handle immediately. If another spawn fails, it signals shutdown and joins all existing workers before reporting that failure. A private spawn seam injects OS-boundary failures while keeping the real worker and cleanup orchestration under test; OS resource exhaustion itself is not induced.
- Shutdown joins all workers before propagating task failures. One failure retains its original panic payload. Multiple failures transfer all payloads in join order as `Vec<Box<dyn std::any::Any + Send>>`; shutdown must not run arbitrary payload destructors during propagation. The caller owns disposal after catching the panic.
- If startup and worker cleanup fail together, the same vector contains the original `std::io::Error` first, then worker panic payloads. No failure is silently replaced by a later cleanup error.
- The queue mutex is never held during callbacks or joins. A callback panic does not poison queue state. Poisoning is rejected, not silently recovered; the public API exposes no caller-controlled code under this mutex.

Regression tests cover partial startup, combined startup/join failures, and panic-payload ownership. Loom covers the normal queue/wakeup/drain protocol only; aborts, allocation failure, foreign exceptions, and arbitrary caller payload disposal remain outside those models.

## Whole-repository review scope

Baseline: clean `67b6cf1a70791912a6812c350cfc73540a06f660`. Review covers Homework 6 only, not the other homework repositories. No Git mutation, commit, push, shared-rule edit, or central-report edit was performed.

All 11 tracked files were read: `.gitignore`, `ASSIGNMENT.md`, `Cargo.lock`, `Cargo.toml`, `Makefile`, `README.md`, `REPORT.md`, `src/lib.rs`, `tests/lifecycle.rs`, `tests/loom_thread_pool.rs`, and `tests/thread_pool.rs`. There were no untracked source files or repository-local rule overrides. Generated `target/` contents and Git internals are excluded.

Standards sources were freshly read from `../../.rules`, `../.rules`, and `../.clinerules/README.md`, plus canonical files 01–08, 11, and 13. The Standards and Spec passes were separate, sequential, read-only passes before functional fixes. Spec sources: `ASSIGNMENT.md` (primary educational contract), then the repository's API documentation, README, report, and executable tests. Generic production preferences do not override the assignment's fixed `fn(i64)` API, mutex/condvar design, or explicit shutdown.

### Standards findings and disposition

Locations below refer to baseline lines, before edits.

| Finding | Severity | Evidence | Rules | Disposition |
| --- | --- | --- | --- | --- |
| Partial constructor failure detached already-created workers waiting forever | P2 | `src/lib.rs:78–83` | 28.3, 30.10, 38.12 | Fixed: retain handles immediately; stop and join before reporting spawn failure |
| Secondary panic payloads were destroyed during the first panic's unwind, losing failures and risking double-panic abort | P1 | `src/lib.rs:119–128` | 9.9, 17.11 | Fixed: transfer all payloads on multiple failures; preserve the original payload for a single failure |
| Test helpers detached a shutdown thread, discarded send failure, resumed after poison without validation, and lacked a local unwind-safety justification | P2 | `tests/thread_pool.rs:13,23,52–60`; `tests/lifecycle.rs:77–78` | 9.10–9.11, 17.9, 28.3 | Fixed: join/check results, reject poison, justify the consuming unwind boundary |
| Documented verification omitted locked resolution and mandatory opt-in error-discard lints | P2 | `Makefile:6–7`; `README.md:18–21` | 35.9, 40.1, 40.10 | Fixed: README and Makefile use the complete locked gates |
| Library MSRV was unspecified | P3 | `Cargo.toml:1–4` | 2.3 | Fixed: declare Rust 1.85; verify the installed 1.85.0 GNU toolchain |
| Public library workflow had no compilable usage example | P3 | `src/lib.rs:33–39` | 4.10, 36.3 | Fixed: add a doctest, verified in normal and Loom modes |

No additional actionable Fowler-smell finding was established. The private spawn seam exists only to inject OS-boundary failures; no alternative executor architecture or public task abstraction was added.

### Spec findings and disposition

No confirmed assignment violation for finite, non-panicking tasks. `ASSIGNMENT.md:34–40` requires positive worker count, fixed workers, enqueue/wakeup, condvar waiting, graceful joins, queue draining, and exactly-once invocation. The algorithm and original tests satisfy these requirements. FIFO is optional (`ASSIGNMENT.md:179–189`); only dequeue order, not concurrent callback completion order, is promised.

The assignment does not specify recovery after task panic, stuck callbacks, resource exhaustion, or implicit shutdown on drop. Those are documented boundaries, not grounds for replacing the educational API. The multi-panic extension changes intentionally: previously shutdown attempted to resume only the first failure and destroyed the rest; now multiple failures are retained in a vector payload. Single-panic behavior and normal task behavior are preserved.

## Verification

Review environment: Windows, `x86_64-pc-windows-gnu`; Rust/Cargo 1.99.1, plus installed Rust/Cargo 1.85.0 for MSRV verification. Edition 2024, optional Loom 0.7.2, no async runtime or new dependency. Compilation occasionally reported an incremental-cache finalization note (`Access is denied. (os error 5)`); commands still completed successfully. No test or model command timed out.

### Baseline and final gates

| Command | Baseline | Final |
| --- | --- | --- |
| `cargo fmt --all --check` | Pass | Pass |
| `cargo check --locked --all-targets` | Pass | Pass |
| `cargo clippy --locked --all-targets -- -D warnings -D unused_must_use -D clippy::let_underscore_must_use -D clippy::unused_result_ok` | Fail: `tests/thread_pool.rs:54`, `unused_result_ok` | Pass |
| `cargo test --locked` | 12 tests, 0 doctests passed | 15 tests (2 unit + 13 integration), 1 doctest passed |
| `cargo test --locked --features loom --test loom_thread_pool --release -- --test-threads=1` | 6 passed | 6 passed |

Baseline fmt/check/Clippy used a 120-second command deadline; baseline ordinary tests used 120 seconds and Loom used 180 seconds. Final combined gates used a 180-second command deadline. Commands and test counts above are actual local results, not CI results.

Additional final checks passed:

```sh
cargo test --locked --release
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings -D unused_must_use -D clippy::let_underscore_must_use -D clippy::unused_result_ok
cargo test --locked --features loom --test loom_thread_pool -- --test-threads=1
cargo test --locked --features loom --doc
cargo +1.85.0-x86_64-pc-windows-gnu check --locked --all-targets --all-features
cargo +1.85.0-x86_64-pc-windows-gnu test --locked
cargo +1.85.0-x86_64-pc-windows-gnu test --locked --features loom --test loom_thread_pool --release -- --test-threads=1
```

The ordinary release run also passed 15 tests and 1 doctest. MSRV ordinary tests passed with the same 15 tests and 1 doctest; MSRV Loom passed all 6 models. Feature-mode doctest passed once on Rust 1.99.1. These checks used explicit command deadlines of 120 or 180 seconds. No repository CI configuration was present and remote CI was not inspected. The Makefile commands were executed directly; no separate `make` invocation is claimed.

### Red/green evidence

Both commands below used a 30-second red deadline, then were rerun unchanged for green within a 60-second combined deadline:

```sh
cargo test --locked --lib failed_startup_ -- --test-threads=1
cargo test --locked --test lifecycle shutdown_retains_all_worker_panics_without_dropping_payloads_while_unwinding -- --exact
```

- Startup red: both tests failed with `left: 0`, `right: 2`: existing workers were not stopped/joined before constructor failure. The combined-failure test injects worker failures as well as the third spawn error. Green: 2 passed; the combined payload retains the initiating `std::io::Error` and both worker failures.
- Shutdown red: `shutdown must not destroy secondary panic payloads during unwinding`. A non-panicking observing destructor detected `thread::panicking()`, exposing the destruction point without deliberately aborting the test process. Green: 1 passed; both original payloads reach the caller.
- Startup red ran against the baseline lifecycle algorithm after adding the private fallible spawn seam as test support; it did not run against byte-identical HEAD. No constructor cleanup fix was present yet. Shutdown red likewise preceded its functional fix. Tests and assertions were unchanged between each focused red and green run; later rustfmt changes were formatting only.
- The original Clippy failure is the red evidence for the misleading `.ok()` in the helper; no production behavior was changed to silence that lint.

The final error audit traced spawn errors, every worker join result, panic payload ownership, queue-lock rejection, and test notification failures. No cleanup error is discarded in the changed production paths. Multiple startup/cleanup failures retain the initiating error first. `AssertUnwindSafe` is confined to test boundaries consuming the pool without reuse; it is not used to recover mutable production state.

### Loom scope

The six models check normal queue/drain behavior: exact arguments with one worker, competing workers without duplicate work, idle-worker wakeup, two idle-worker joins, submission followed by shutdown, and submitter completion before shutdown. They execute the real implementation through Loom's `Arc`, `Mutex`, `Condvar`, and thread replacements.

The two-worker/two-task model has `preemption_bound = Some(2)`; the other five retain Loom defaults. No `LOOM_*` override among the inspected exploration settings was present, and no permutation or duration cap was introduced. External command deadlines bound execution rather than silently truncating a successful model. These checks do not establish correctness for arbitrary workloads, OS failures, task panics, poisoning, aborts, allocation failures, or foreign exceptions. No statistical benchmark or performance claim is made.

## Limitations

This is an educational pool, not a production executor. The unbounded queue requires finite work; callbacks must terminate and must not depend on an unavailable worker in the same pool. There is no timeout, cancellation, task restart, or automatic shutdown in `Drop`. All normal accepted work drains exactly once only when callbacks finish without panicking; queued work may remain after worker failures. Queue poisoning is rejected, not repaired, and is not injected by these public-boundary tests because no caller-controlled code runs under the internal queue mutex.

OS thread-creation errors are injected at the spawn boundary, not by exhausting system resources. Panic-payload destruction during unwind is observed safely; actual process-abort behavior is not exercised. Catching callers must handle payload disposal themselves; abort strategy, foreign exceptions, and additional panics during caller cleanup can still terminate the process. The tracked lockfile pins only this dependency graph, not arbitrary downstream resolution or native toolchains.
