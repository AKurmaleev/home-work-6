#![forbid(unsafe_code)]

use std::collections::VecDeque;

#[cfg(feature = "loom")]
pub mod sync {
    pub use loom::sync::{Arc, Condvar, Mutex};
    pub use loom::thread;
}

#[cfg(not(feature = "loom"))]
pub mod sync {
    pub use std::sync::{Arc, Condvar, Mutex};
    pub use std::thread;
}

use sync::{Arc, Condvar, Mutex, thread};

/// The fixed function invoked with each submitted argument.
/// Tasks must finish without panicking for the drain guarantee to hold.
pub type Task = fn(i64);

struct State {
    queue: VecDeque<i64>,
    shutting_down: bool,
}

struct Shared {
    state: Mutex<State>,
    has_work: Condvar,
}

/// A fixed-size pool for finite workloads, with a FIFO argument queue.
/// Call `shutdown` explicitly: dropping the pool does not stop its workers.
/// Queue capacity is not bounded; this is not a production admission-control API.
pub struct ThreadPool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
}

fn run_worker(shared: Arc<Shared>, task: Task) {
    loop {
        let num = {
            let mut state = shared
                .state
                .lock()
                .expect("queue state must not be poisoned");
            while state.queue.is_empty() && !state.shutting_down {
                state = shared
                    .has_work
                    .wait(state)
                    .expect("queue state must not be poisoned while waiting");
            }
            // Drain queued work even after shutdown was requested.
            state.queue.pop_front()
        };
        match num {
            Some(num) => task(num),
            None => return,
        }
    }
}

impl ThreadPool {
    /// Creates `worker_count` workers running the same task function.
    ///
    /// # Panics
    /// Panics if `worker_count` is zero or the OS cannot start a worker.
    pub fn new(worker_count: usize, task: Task) -> Self {
        assert!(worker_count > 0, "worker_count must be positive");
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                shutting_down: false,
            }),
            has_work: Condvar::new(),
        });
        let workers = (0..worker_count)
            .map(|_| {
                let shared = Arc::clone(&shared);
                thread::spawn(move || run_worker(shared, task))
            })
            .collect();
        Self { shared, workers }
    }

    /// Enqueues one argument and wakes a worker; safe for concurrent callers.
    /// Shutdown consumes the pool, so submissions cannot follow shutdown.
    ///
    /// # Panics
    /// Panics if the queue mutex is poisoned.
    pub fn execute(&self, num: i64) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("queue state must not be poisoned");
        state.queue.push_back(num);
        self.shared.has_work.notify_one();
    }

    /// Drains submitted work, wakes all idle workers, and joins every worker.
    /// Tasks must terminate; no timeout or task cancellation is provided.
    ///
    /// # Panics
    /// Panics if the queue mutex is poisoned. If a task panicked, joins all
    /// workers before resuming the first worker panic; queued work may be lost.
    pub fn shutdown(self) {
        {
            let mut state = self
                .shared
                .state
                .lock()
                .expect("queue state must not be poisoned");
            state.shutting_down = true;
            self.shared.has_work.notify_all();
        }

        // Retain panic payloads until all workers have been joined: even a
        // user-defined payload destructor must not interrupt the join loop.
        let mut panics = Vec::new();
        for worker in self.workers {
            if let Err(payload) = worker.join() {
                panics.push(payload);
            }
        }
        if !panics.is_empty() {
            std::panic::resume_unwind(panics.remove(0));
        }
    }
}
