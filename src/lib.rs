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
///
/// ```
/// use synchronization_thread_pool::ThreadPool;
///
/// # fn example() {
/// fn task(num: i64) {
///     assert_eq!(num, 42);
/// }
/// let pool = ThreadPool::new(2, task);
/// pool.execute(42);
/// pool.shutdown();
/// # }
/// # #[cfg(feature = "loom")]
/// # loom::model(example);
/// # #[cfg(not(feature = "loom"))]
/// # example();
/// ```
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
    /// Panics if `worker_count` is zero, handle storage exceeds addressable
    /// capacity, or the OS cannot start a worker.
    /// On a spawn error, stops and joins already-started workers first.
    /// If those workers also panic, the panic payload is a
    /// `Vec<Box<dyn std::any::Any + Send>>` containing the spawn `std::io::Error`
    /// first, then worker payloads in join order; otherwise it is an error message.
    pub fn new(worker_count: usize, task: Task) -> Self {
        Self::new_with_spawn(worker_count, task, |shared, task| {
            thread::Builder::new().spawn(move || run_worker(shared, task))
        })
    }

    fn new_with_spawn(
        worker_count: usize,
        task: Task,
        mut spawn: impl FnMut(Arc<Shared>, Task) -> std::io::Result<thread::JoinHandle<()>>,
    ) -> Self {
        assert!(worker_count > 0, "worker_count must be positive");
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                shutting_down: false,
            }),
            has_work: Condvar::new(),
        });
        let mut pool = Self {
            shared,
            // Allocate handle storage before any worker can need cleanup.
            workers: Vec::with_capacity(worker_count),
        };
        for _ in 0..worker_count {
            match spawn(Arc::clone(&pool.shared), task) {
                Ok(worker) => pool.workers.push(worker),
                Err(error) => {
                    let mut panics = pool.stop_and_join();
                    if panics.is_empty() {
                        panic!("failed to start worker: {error}");
                    }
                    panics.insert(0, Box::new(error));
                    std::panic::resume_unwind(Box::new(panics));
                }
            }
        }
        pool
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
    /// workers before propagating the failure; queued work may be lost.
    /// One worker panic retains its original payload. Multiple worker panics
    /// are propagated as `Vec<Box<dyn std::any::Any + Send>>` in join order,
    /// transferring every payload to the caller without dropping any during
    /// unwinding. A caller catching the panic is responsible for payload disposal.
    /// Panic propagation requires unwinding; `panic = "abort"` terminates instead.
    pub fn shutdown(self) {
        let mut panics = self.stop_and_join();
        if panics.len() == 1 {
            std::panic::resume_unwind(panics.remove(0));
        }
        if !panics.is_empty() {
            std::panic::resume_unwind(Box::new(panics));
        }
    }

    fn stop_and_join(self) -> Vec<Box<dyn std::any::Any + Send>> {
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
        panics
    }
}

#[cfg(all(test, not(feature = "loom")))]
mod tests {
    use super::*;
    use std::any::Any;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc;
    use std::time::Duration;

    fn noop() {}

    fn panic_on_exit() {
        panic!("injected cleanup failure");
    }

    fn observe_failed_startup(on_exit: fn()) -> (Box<dyn Any + Send>, usize) {
        let (finished_tx, finished_rx) = mpsc::channel();
        let mut observed_shared = None;
        let mut attempts = 0;
        // Constructor ownership is consumed on failure; no pool is reused.
        let result = catch_unwind(AssertUnwindSafe(|| {
            ThreadPool::new_with_spawn(
                3,
                |_| {},
                |shared, task| {
                    attempts += 1;
                    if attempts == 3 {
                        return Err(std::io::Error::other("injected spawn failure"));
                    }
                    observed_shared = Some(Arc::clone(&shared));
                    let finished_tx = finished_tx.clone();
                    thread::Builder::new().spawn(move || {
                        run_worker(shared, task);
                        finished_tx.send(()).unwrap();
                        on_exit();
                    })
                },
            )
        }));
        let completed_before_cleanup = finished_rx.try_iter().count();

        // Release detached workers even on the unfixed implementation, so red
        // runs do not leave idle workers behind for the rest of the test suite.
        let shared = observed_shared.expect("two workers must have been started");
        {
            let mut state = shared.state.lock().unwrap();
            state.shutting_down = true;
            shared.has_work.notify_all();
        }
        for _ in completed_before_cleanup..2 {
            finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        match result {
            Err(payload) => (payload, completed_before_cleanup),
            Ok(pool) => {
                pool.shutdown();
                panic!("injected spawn failure must reject construction");
            }
        }
    }

    #[test]
    fn failed_startup_joins_previously_started_workers() {
        let (failure, completed) = observe_failed_startup(noop);
        assert_eq!(
            completed, 2,
            "startup failure must stop and join existing workers"
        );
        let message = failure.downcast_ref::<String>().unwrap();
        assert!(message.contains("injected spawn failure"));
    }

    #[test]
    fn failed_startup_retains_spawn_error_and_cleanup_panics() {
        let (failure, completed) = observe_failed_startup(panic_on_exit);
        assert_eq!(
            completed, 2,
            "startup failure must attempt every worker join"
        );
        let failures = failure.downcast::<Vec<Box<dyn Any + Send>>>().unwrap();
        assert_eq!(failures.len(), 3);
        let error = failures[0].downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(error.to_string(), "injected spawn failure");
        for payload in &failures[1..] {
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&"injected cleanup failure")
            );
        }
    }
}
