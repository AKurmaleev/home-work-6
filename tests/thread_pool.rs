#![cfg(not(feature = "loom"))]

use std::ops::Range;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use synchronization_thread_pool::ThreadPool;

static EVENTS: Mutex<Vec<i64>> = Mutex::new(Vec::new());

fn record_task(num: i64) {
    let mut events = EVENTS.lock().unwrap();
    events.push(num);
}

fn slow_record_task(num: i64) {
    thread::sleep(Duration::from_millis(10));
    record_task(num);
}

fn values_in(range: Range<i64>) -> Vec<i64> {
    let events = EVENTS.lock().unwrap();
    events
        .iter()
        .copied()
        .filter(|num| range.contains(num))
        .collect()
}

fn count_in(range: Range<i64>) -> usize {
    values_in(range).len()
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if condition() {
            return true;
        }

        thread::sleep(Duration::from_millis(2));
    }

    condition()
}

fn assert_shutdown_finishes(pool: ThreadPool) {
    let (tx, rx) = std::sync::mpsc::channel();

    let shutdown = thread::spawn(move || {
        pool.shutdown();
        tx.send(())
            .expect("shutdown test receiver must remain connected");
    });

    assert!(
        rx.recv_timeout(Duration::from_secs(2)).is_ok(),
        "shutdown did not finish; workers may be stuck on the Condvar"
    );
    shutdown
        .join()
        .expect("shutdown test thread must not panic");
}

#[test]
fn new_rejects_zero_workers() {
    let result = std::panic::catch_unwind(|| ThreadPool::new(0, record_task));
    assert!(result.is_err(), "ThreadPool::new(0, _) must panic");
}

#[test]
fn single_worker_runs_one_task() {
    let pool = ThreadPool::new(1, record_task);

    pool.execute(1_000);

    assert!(
        wait_until(Duration::from_secs(1), || count_in(1_000..1_001) == 1),
        "worker did not execute the submitted task"
    );

    pool.shutdown();
}

#[test]
fn workers_wake_after_being_idle() {
    let pool = ThreadPool::new(2, record_task);

    thread::sleep(Duration::from_millis(30));
    pool.execute(2_000);
    pool.execute(2_001);

    assert!(
        wait_until(Duration::from_secs(1), || count_in(2_000..2_002) == 2),
        "idle workers were not woken by Condvar notifications"
    );

    pool.shutdown();
}

#[test]
fn shutdown_unblocks_idle_workers() {
    let pool = ThreadPool::new(4, record_task);
    assert_shutdown_finishes(pool);
}

#[test]
fn shutdown_waits_for_slow_running_tasks() {
    let pool = ThreadPool::new(2, slow_record_task);

    for num in 3_000..3_006 {
        pool.execute(num);
    }

    pool.shutdown();

    let mut values = values_in(3_000..3_006);
    values.sort_unstable();

    assert_eq!(
        values,
        (3_000..3_006).collect::<Vec<_>>(),
        "shutdown returned before all queued slow tasks finished"
    );
}

#[test]
fn many_tasks_are_executed_exactly_once() {
    let pool = ThreadPool::new(4, record_task);

    for num in 4_000..4_100 {
        pool.execute(num);
    }

    pool.shutdown();

    let mut values = values_in(4_000..4_100);
    values.sort_unstable();

    assert_eq!(values.len(), 100);
    assert_eq!(
        values,
        (4_000..4_100).collect::<Vec<_>>(),
        "tasks must not be lost or executed twice"
    );
}

#[test]
fn concurrent_submitters_do_not_lose_tasks() {
    let pool = ThreadPool::new(4, record_task);

    thread::scope(|scope| {
        for producer in 0..4 {
            let pool = &pool;

            scope.spawn(move || {
                let start = 5_000 + producer * 100;

                for num in start..start + 25 {
                    pool.execute(num);
                }
            });
        }
    });

    pool.shutdown();

    for producer in 0..4 {
        let start = 5_000 + producer * 100;
        let mut values = values_in(start..start + 25);
        values.sort_unstable();

        assert_eq!(
            values,
            (start..start + 25).collect::<Vec<_>>(),
            "tasks from producer {producer} were lost or duplicated"
        );
    }
}

#[test]
fn tasks_can_be_submitted_in_bursts() {
    let pool = ThreadPool::new(3, record_task);

    for num in 6_000..6_010 {
        pool.execute(num);
    }

    assert!(
        wait_until(Duration::from_secs(1), || count_in(6_000..6_010) == 10),
        "first burst did not complete"
    );

    for num in 6_010..6_020 {
        pool.execute(num);
    }

    pool.shutdown();

    let mut values = values_in(6_000..6_020);
    values.sort_unstable();

    assert_eq!(
        values,
        (6_000..6_020).collect::<Vec<_>>(),
        "burst submissions should all be processed"
    );
}

#[test]
fn negative_numbers_are_valid_task_arguments() {
    let pool = ThreadPool::new(2, record_task);

    for num in -100..-90 {
        pool.execute(num);
    }

    pool.shutdown();

    let mut values = values_in(-100..-90);
    values.sort_unstable();

    assert_eq!(values, (-100..-90).collect::<Vec<_>>());
}
