# Домашнее задание: Thread Pool на Mutex и Condvar

## Цель

Нужно реализовать простой thread pool:

- есть фиксированная функция-задача вида `fn task(num: i64)`;
- пользователь кладет в пул числа через `ThreadPool::execute(num)`;
- воркеры забирают числа из общей очереди и вызывают `task(num)`;
- очередь задач должна быть защищена `Mutex`;
- воркеры должны засыпать на `Condvar`, когда задач нет;
- `shutdown(self)` должен дождаться завершения всех уже отправленных задач и аккуратно остановить воркеры.

В этом проекте уже задан публичный API в `src/lib.rs` и набор интеграционных тестов в `tests/`.

## Что нужно реализовать

Откройте `src/lib.rs` и замените `todo!()` на рабочую реализацию.

Ожидаемый API:

```rust
pub type Task = fn(i64);

impl ThreadPool {
    pub fn new(worker_count: usize, task: Task) -> Self;
    pub fn execute(&self, num: i64);
    pub fn shutdown(self);
}
```

Поведение:

1. `ThreadPool::new(0, task)` должен паниковать.
2. `ThreadPool::new(n, task)` создает `n` worker-потоков.
3. `execute(num)` добавляет новую работу в очередь и будит хотя бы одного воркера.
4. Если очередь пуста, воркер не крутится в цикле, а спит на `Condvar`.
5. `shutdown(self)` запрещает дальнейшую работу, будит всех воркеров и делает `join` каждого worker-потока.
6. Если к моменту `shutdown` в очереди еще есть задачи, они должны быть выполнены до выхода воркеров.
7. Все отправленные задачи должны быть выполнены ровно один раз.

## Рекомендуемая структура

Сделайте общее состояние, которое будет жить в `Arc`:

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

Так как функция-задача фиксирована, в очереди достаточно хранить аргументы `i64`. Если хочется буквально сделать `Mutex<Vec<Task>>`, заведите отдельную структуру:

```rust
struct TaskItem {
    num: i64,
}
```

Тогда `State` будет хранить `queue: Vec<TaskItem>`, а worker будет доставать `TaskItem` и вызывать `task(item.num)`.

Сам `ThreadPool` может хранить:

```rust
pub struct ThreadPool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
    task: Task,
}
```

Можно хранить `task` внутри `Shared`, чтобы воркерам было проще получить к нему доступ. Выберите вариант, который вам понятнее.

## Подсказки

Импорты для обычной реализации:

```rust
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
```

В этом проекте уже есть модуль `sync` в `src/lib.rs`. Если хотите, используйте его вместо прямых импортов:

```rust
use crate::sync::{thread, Arc, Condvar, Mutex};
```

Так решение будет проще прогонять через `loom`.

Создание потока:

```rust
let handle = thread::spawn(move || {
    // код worker-а
});
```

Ожидание потока:

```rust
handle.join().expect("worker panicked");
```

Добавление задачи:

```rust
let mut state = shared.state.lock().expect("mutex poisoned");
state.queue.push(num);
shared.has_work.notify_one();
```

Ожидание на `Condvar` почти всегда должно быть в цикле:

```rust
let mut state = shared.state.lock().expect("mutex poisoned");

while state.queue.is_empty() && !state.shutting_down {
    state = shared
        .has_work
        .wait(state)
        .expect("mutex poisoned while waiting");
}
```

Почему нужен `while`, а не `if`: поток может проснуться без новой задачи, или другой worker может забрать задачу раньше.

Типичный worker-цикл:

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

Важная деталь: задачу нужно выполнять после выхода из критической секции. Не держите mutex во время `task(num)`, иначе один долгий task заблокирует всю очередь.

Для `shutdown`:

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

`notify_all` нужен, потому что на момент завершения несколько воркеров могут спать на `Condvar`.

## Про порядок задач

Если использовать `Vec<i64>` и `pop()`, задачи будут выполняться в порядке LIFO. В этом задании FIFO-порядок не требуется. Важно только, чтобы каждая задача была выполнена ровно один раз.

Если хотите FIFO, можно использовать `VecDeque<i64>` из стандартной библиотеки:

```rust
use std::collections::VecDeque;
```

Но базовый вариант с `Vec<i64>` полностью подходит для задания.

## Как запускать

Обычные тесты:

```bash
cargo test
```

Только интеграционные тесты thread pool:

```bash
cargo test --test thread_pool
```

Loom-тесты:

```bash
cargo test --features loom --test loom_thread_pool
```

Если loom найдет ошибку, он обычно покажет один конкретный interleaving, на котором код ломается или зависает. Сначала исправьте обычные тесты, потом запускайте loom.

Запустить примерный проект как библиотеку нельзя, потому что это `lib`-crate. Для ручной проверки можно временно добавить `src/main.rs` или написать маленький тест в `tests/`.

Форматирование:

```bash
cargo fmt
```

Проверка компиляции без запуска тестов:

```bash
cargo test --no-run
```

## Что сдавать

Сдайте измененный `src/lib.rs`, в котором:

- нет `todo!()`;
- нет busy waiting;
- нет `unsafe`;
- обычные тесты проходят;
- loom-тесты проходят с `--features loom`.
