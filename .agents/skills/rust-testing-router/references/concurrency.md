# Concurrency Testing

> Use `loom` for lock-free or custom synchronization code; stress tests are not proof.

## When to Use

Reach for `loom` when hand-writing raw atomics, lock-free data structures, or custom synchronization where an `Ordering` mistake only appears under rare interleavings.

Do not use it for ordinary logic around standard `Mutex`, `RwLock`, or channels unless you are testing a custom primitive built from them.

## Pattern

Gate instrumented types behind `#[cfg(loom)]`; production code keeps using `std`.

```rust
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(loom))]
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Flag(AtomicBool);

impl Flag {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn set(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
```

```rust
#[cfg(loom)]
#[test]
fn flag_set_visible_to_other_thread() {
    loom::model(|| {
        let flag = loom::sync::Arc::new(Flag::new());
        let flag2 = loom::sync::Arc::clone(&flag);

        let writer = loom::thread::spawn(move || flag2.set());
        writer.join().unwrap();

        assert!(flag.is_set(), "flag must be set after join");
    });
}
```

Run it with:

```bash
RUSTFLAGS="--cfg loom" cargo test --test loom_flag
```

Keep loom closures small; interleavings grow combinatorially. `loom` checks the C11 memory model, not unrelated business logic.
