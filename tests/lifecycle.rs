#![cfg(not(feature = "loom"))]

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::Duration;

use synchronization_thread_pool::ThreadPool;

// A single test owns this rendezvous; timeout prevents a broken pool hanging it.
static RENDEZVOUS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

fn rendezvous_task(_: i64) {
    let (lock, ready) = &RENDEZVOUS;
    let mut arrived = lock.lock().unwrap();
    *arrived += 1;
    ready.notify_all();
    let (arrived, _) = ready
        .wait_timeout_while(arrived, Duration::from_secs(2), |count| *count < 2)
        .unwrap();
    assert_eq!(
        *arrived, 2,
        "tasks must run concurrently outside the queue lock"
    );
}

#[test]
fn two_workers_execute_callbacks_outside_the_queue_lock() {
    let pool = ThreadPool::new(2, rendezvous_task);
    pool.execute(1);
    pool.execute(2);
    pool.shutdown();
}

static EVENTS: Mutex<Vec<i64>> = Mutex::new(Vec::new());

fn record(num: i64) {
    EVENTS.lock().unwrap().push(num);
}

#[test]
fn one_worker_dequeues_fifo_and_keeps_repeated_values() {
    let input = [i64::MIN, 7, 7, i64::MAX];
    let pool = ThreadPool::new(1, record);
    for num in input {
        pool.execute(num);
    }
    pool.shutdown();
    assert_eq!(*EVENTS.lock().unwrap(), input);
}

static PANIC_GATE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
static SECOND_DONE: Mutex<bool> = Mutex::new(false);

fn panic_then_release_other_worker(num: i64) {
    if num == 1 {
        let (lock, gate) = &PANIC_GATE;
        *lock.lock().unwrap() = true;
        gate.notify_all();
        panic!("intentional task failure");
    }
    let (lock, gate) = &PANIC_GATE;
    let (opened, _) = gate
        .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(2), |opened| {
            !*opened
        })
        .unwrap();
    assert!(*opened, "panicking task must have run");
    *SECOND_DONE.lock().unwrap() = true;
}

#[test]
fn shutdown_reports_worker_panic_after_joining_other_workers() {
    let pool = ThreadPool::new(2, panic_then_release_other_worker);
    pool.execute(2);
    pool.execute(1);
    let (tx, rx) = mpsc::channel();
    let shutdown = std::thread::spawn(move || {
        // The pool is consumed and never reused after this unwind boundary.
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.shutdown()))
            .expect_err("one worker must have panicked");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"intentional task failure"),
            "a single worker panic must retain its original payload"
        );
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    shutdown.join().unwrap();
    assert!(*SECOND_DONE.lock().unwrap());
}

static PAYLOAD_DROPPED_WHILE_UNWINDING: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct PanicPayload(i64);

impl Drop for PanicPayload {
    fn drop(&mut self) {
        // Observation only: a real panicking destructor here would abort the
        // process if shutdown dropped this payload during another unwind.
        PAYLOAD_DROPPED_WHILE_UNWINDING.fetch_or(std::thread::panicking(), Ordering::Relaxed);
    }
}

fn panic_with_payload(num: i64) {
    std::panic::panic_any(PanicPayload(num));
}

#[test]
fn shutdown_retains_all_worker_panics_without_dropping_payloads_while_unwinding() {
    let pool = ThreadPool::new(2, panic_with_payload);
    pool.execute(1);
    pool.execute(2);
    // Shutdown consumes the pool; captured handles/state are not reused after
    // unwinding, and both callback failures belong to this runtime boundary.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.shutdown()))
        .expect_err("both workers must have panicked");
    assert!(
        !PAYLOAD_DROPPED_WHILE_UNWINDING.load(Ordering::Relaxed),
        "shutdown must not destroy secondary panic payloads during unwinding"
    );
    let failures = failure.downcast::<Vec<Box<dyn Any + Send>>>().unwrap();
    let mut arguments: Vec<_> = failures
        .iter()
        .map(|payload| payload.downcast_ref::<PanicPayload>().unwrap().0)
        .collect();
    arguments.sort_unstable();
    assert_eq!(
        arguments,
        [1, 2],
        "every worker failure must reach the caller"
    );
}
