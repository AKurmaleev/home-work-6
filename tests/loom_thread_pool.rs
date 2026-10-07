use synchronization_thread_pool::ThreadPool;

loom::lazy_static! {
    static ref PROCESSED: loom::sync::Mutex<Vec<i64>> = loom::sync::Mutex::new(Vec::new());
}

fn record_task(num: i64) {
    PROCESSED.lock().unwrap().push(num);
}

#[test]
fn loom_shutdown_drains_each_argument_exactly_once() {
    loom::model(|| {
        let pool = ThreadPool::new(1, record_task);
        pool.execute(10);
        pool.execute(20);
        pool.shutdown();
        assert_eq!(*PROCESSED.lock().unwrap(), vec![10, 20]);
    });
}

#[test]
fn loom_two_idle_workers_are_both_joined() {
    loom::model(|| {
        let pool = ThreadPool::new(2, noop_task);
        pool.shutdown();
    });
}

#[test]
fn loom_competing_workers_do_not_duplicate_work() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    model.check(|| {
        let pool = ThreadPool::new(2, record_task);
        pool.execute(10);
        pool.execute(20);
        pool.shutdown();
        let mut actual = PROCESSED.lock().unwrap().clone();
        actual.sort_unstable();
        assert_eq!(actual, vec![10, 20]);
    });
}

fn noop_task(_: i64) {}

#[test]
fn loom_shutdown_wakes_idle_worker() {
    loom::model(|| {
        let pool = ThreadPool::new(1, noop_task);
        pool.shutdown();
    });
}

#[test]
fn loom_submitted_work_does_not_deadlock_shutdown() {
    loom::model(|| {
        let pool = ThreadPool::new(1, noop_task);

        pool.execute(10);
        pool.execute(20);
        pool.shutdown();
    });
}

#[test]
fn loom_submitter_can_finish_before_shutdown() {
    loom::model(|| {
        let pool = loom::sync::Arc::new(ThreadPool::new(1, noop_task));

        let submitter = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.execute(30))
        };

        submitter.join().expect("submitter panicked");

        let pool = loom::sync::Arc::try_unwrap(pool)
            .ok()
            .expect("pool still shared");
        pool.shutdown();
    });
}
