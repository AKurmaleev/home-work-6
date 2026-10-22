# Homework: Thread Pool with Mutex and Condvar

## Goal

Implement a simple thread pool:

- there is a fixed task function of the form `fn task(num: i64)`;
- the user submits numbers to the pool through `ThreadPool::execute(num)`;
- workers take numbers from a shared queue and call `task(num)`;
- the task queue must be protected by a `Mutex`;
- workers must sleep on a `Condvar` when there are no tasks;
- `shutdown(self)` must wait for all previously submitted tasks to finish and stop the workers gracefully.

This project already provides a public API in `src/lib.rs` and a set of integration tests in `tests/`.

## What to implement

Open `src/lib.rs` and replace `todo!()` with a working implementation.

Expected API:

```rust
pub type Task = fn(i64);

impl ThreadPool {
    pub fn new(worker_count: usize, task: Task) -> Self;
    pub fn execute(&self, num: i64);
    pub fn shutdown(self);
}
```

Behavior:

1. `ThreadPool::new(0, task)` must panic.
2. `ThreadPool::new(n, task)` creates `n` worker threads.
3. `execute(num)` adds new work to the queue and wakes at least one worker.
4. If the queue is empty, a worker sleeps on a `Condvar` instead of spinning in a loop.
5. `shutdown(self)` prevents further work, wakes all workers, and joins each worker thread.
6. If tasks remain in the queue when `shutdown` is called, they must be executed before the workers exit.
7. All submitted tasks must be executed exactly once.

## Recommended structure

Create shared state that will live in an `Arc`:

```rust
struct Shared {
    state: Mutex<State>,
    has_work: Condvar,
}

struct State {
    queue: Vec<i64>,
    shutting_down: bool,
}
```

Since the task function is fixed, storing `i64` arguments in the queue is sufficient. If you want to literally use `Mutex<Vec<Task>>`, introduce a separate struct:

```rust
struct TaskItem {
    num: i64,
}
```

Then `State` will store `queue: Vec<TaskItem>`, and a worker will take a `TaskItem` and call `task(item.num)`.

`ThreadPool` itself can store:

```rust
pub struct ThreadPool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
    task: Task,
}
```

You can store `task` inside `Shared` to make it easier for workers to access. Choose whichever option is clearer to you.

## Hints

Imports for a regular implementation:

```rust
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
```

This project already has a `sync` module in `src/lib.rs`. You can use it instead of direct imports if you prefer:

```rust
use crate::sync::{thread, Arc, Condvar, Mutex};
```

This will make it easier to run the solution through `loom`.

Creating a thread:

```rust
let handle = thread::spawn(move || {
    // worker code
});
```

Waiting for a thread:

```rust
handle.join().expect("worker panicked");
```

Adding a task:

```rust
let mut state = shared.state.lock().expect("mutex poisoned");
state.queue.push(num);
shared.has_work.notify_one();
```

Waiting on a `Condvar` should almost always happen in a loop:

```rust
let mut state = shared.state.lock().expect("mutex poisoned");

while state.queue.is_empty() && !state.shutting_down {
    state = shared
        .has_work
        .wait(state)
        .expect("mutex poisoned while waiting");
}
```

Why `while` is needed instead of `if`: a thread may wake without a new task, or another worker may take the task first.

A typical worker loop:

```rust
loop {
    let maybe_num = {
        let mut state = shared.state.lock().expect("mutex poisoned");

        while state.queue.is_empty() && !state.shutting_down {
            state = shared.has_work.wait(state).expect("mutex poisoned");
        }

        if let Some(num) = state.queue.pop() {
            Some(num)
        } else if state.shutting_down {
            None
        } else {
            continue;
        }
    };

    match maybe_num {
        Some(num) => task(num),
        None => break,
    }
}
```

An important detail: the task must run after leaving the critical section. Do not hold the mutex during `task(num)`, otherwise one long-running task will block the entire queue.

For `shutdown`:

```rust
{
    let mut state = shared.state.lock().expect("mutex poisoned");
    state.shutting_down = true;
    shared.has_work.notify_all();
}

for worker in workers {
    worker.join().expect("worker panicked");
}
```

`notify_all` is needed because several workers may be sleeping on the `Condvar` at shutdown.

## Task order

Using `Vec<i64>` and `pop()` executes tasks in LIFO order. FIFO order is not required by this assignment. The only requirement is that each task be executed exactly once.

If you want FIFO, you can use `VecDeque<i64>` from the standard library:

```rust
use std::collections::VecDeque;
```

However, the basic version with `Vec<i64>` fully satisfies the assignment.

## How to run

Regular tests:

```bash
cargo test
```

Only the thread pool integration tests:

```bash
cargo test --test thread_pool
```

Loom tests:

```bash
cargo test --features loom --test loom_thread_pool
```

If loom finds an error, it usually shows a specific interleaving in which the code breaks or hangs. Get the regular tests passing first, then run loom.

You cannot run the sample project directly because it is a `lib` crate. For manual verification, you can temporarily add `src/main.rs` or write a small test in `tests/`.

Formatting:

```bash
cargo fmt
```

Checking compilation without running tests:

```bash
cargo test --no-run
```

## What to submit

Submit the modified `src/lib.rs` with:

- no `todo!()`;
- no busy waiting;
- no `unsafe`;
- passing regular tests;
- passing loom tests with `--features loom`.
