# Concurrency Testing

> Use `loom` to exhaustively test lock-free and concurrent code — running it a million times isn't proof.

## Why It Matters

A stress test can run a billion iterations under the OS scheduler and still never hit the one thread interleaving that breaks your atomic ordering. `loom` systematically explores every thread scheduling and memory-reordering the C11 memory model permits, turning "we ran it a lot and it seemed fine" into a proof of correctness for every interleaving within the model's bounds. Tokio uses loom to verify its own internal synchronization primitives.

## When to Use

Reach for `loom` when you're hand-writing something built on raw atomics, lock-free data structures, or custom synchronization — code where a subtle `Ordering` mistake would only manifest under a specific, rare interleaving.

Do **not** reach for `loom` when:
- You're just calling `Mutex`/`RwLock`/channels from the standard library or a well-tested crate — those are already loom-tested upstream; test *your* logic around them with ordinary tests instead.
- The bug you're chasing isn't concurrency-related — loom checks the C11 memory model, not general logic correctness.

## Bad

```rust
// Might pass a billion times and still not prove correctness —
// the OS scheduler used in CI may never hit the racy interleaving.
#[test]
fn stress_test_flag() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let flag = Arc::new(AtomicBool::new(false));
    for _ in 0..1_000_000 {
        let flag = Arc::clone(&flag);
        std::thread::spawn(move || {
            flag.store(true, Ordering::Relaxed);
        });
    }
}
```

## Good: Gate Types Behind `#[cfg(loom)]`

Swap in loom's instrumented types only during model checking; production code keeps using `std`:

```rust
// src/flag.rs
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
// tests/loom_flag.rs (or a #[cfg(loom)] mod inside the crate)
#[cfg(loom)]
mod tests {
    use loom::sync::Arc;
    use super::Flag;

    #[test]
    fn flag_set_visible_to_other_thread() {
        loom::model(|| {
            let flag = Arc::new(Flag::new());
            let flag2 = Arc::clone(&flag);

            let writer = loom::thread::spawn(move || {
                flag2.set();
            });

            let seen_before_join = flag.is_set(); // may legitimately be false
            writer.join().unwrap();

            // After join, the writer must have completed.
            assert!(flag.is_set(), "flag must be set after join");
            let _ = seen_before_join;
        });
    }
}
```

Run it with the `loom` cfg flag set:

```bash
RUSTFLAGS="--cfg loom" cargo test --test loom_flag
```

## Key Points

- Keep loom model closures **small**. The number of interleavings explodes combinatorially with the number of atomic operations and threads — test one primitive or one algorithm at a time, not a whole subsystem.
- `loom` replaces `std::sync::atomic`, `std::sync::Mutex`, `std::thread`, and `std::cell` with instrumented equivalents — import from `loom::` only under `#[cfg(loom)]`.
- `loom::model(|| { ... })` is the entry point; loom runs the closure repeatedly under different schedules until it has explored the reachable state space.
- `loom` checks the C11 memory model specifically — it will not catch a logic bug that's unrelated to concurrency.

## See Also

- [`async-testing.md`](async-testing.md) — testing `async fn` behavior, not the primitives underneath it
- [`benchmarking.md`](benchmarking.md) — benchmark concurrent code only after `loom` has verified it's correct
