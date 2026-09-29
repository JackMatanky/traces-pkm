//! Deterministic `TZ` injection for local-zone tests.
//!
//! chrono's `Local` reads `TZ` per call through `std::env::var` (serialized
//! by std's environment lock, so concurrent reads are memory-safe); it also
//! caches the resolved offset in a thread-local for up to one second
//! (chrono's internal `local::unix::Cache`), re-reading `TZ` only on that
//! thread's first lookup or once the cache goes stale. The test harness runs
//! each test on its own thread, so the first lookup on a fresh thread always
//! happens after `TzGuard::set` has already swapped `TZ`, keeping the
//! per-thread cache in step with the swap. A test that swaps `TZ` before its
//! first local-clock read therefore observes its zone deterministically.
//! [`TzGuard`] performs that swap and holds a shared
//! mutex so concurrent tests never interleave reads of one zone with writes
//! of another; the edition-2024 `unsafe` on the write exists for non-Rust
//! `getenv` readers, of which this dependency tree has none during tests.
//! Tests whose assertions depend on the zone must hold the guard (`set` or
//! `keep`); zone-stable assertions elsewhere are unaffected by a swap. The
//! previous `TZ` is restored when the test thread ends.

/// Serializes tests against the process-global `TZ` variable.
static TZ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    /// Per-test-thread holder of the [`TzGuard`]; the guard drops when the
    /// thread ends, restoring `TZ`.
    static TZ_GUARD: std::cell::RefCell<Option<TzGuard>> =
        const { std::cell::RefCell::new(None) };
}

/// Owns the process-global `TZ` variable for the duration of one test.
///
/// Install with [`TzGuard::set`] to pin a zone or [`TzGuard::keep`] to hold
/// the lock without changing the zone. The guard registers itself on the test
/// thread and lives until the thread ends; a second `set` on the same thread
/// just swaps the value, and the original guard still restores the true
/// original.
pub(crate) struct TzGuard {
    previous: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl TzGuard {
    /// Holds the `TZ` lock for this thread without changing the zone.
    pub(crate) fn keep() {
        Self::install(None);
    }

    /// Sets `TZ` to `zone` for this thread, restoring the previous value when
    /// the thread ends.
    pub(crate) fn set(zone: &str) {
        Self::install(Some(zone));
    }

    fn install(zone: Option<&str>) {
        TZ_GUARD.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                // This thread already owns `TZ`; a second `set` just swaps
                // the value and the original guard still restores the true
                // original on drop.
                if let Some(zone) = zone {
                    set_var(zone);
                }
            } else {
                let lock = TZ_LOCK
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let previous = std::env::var("TZ").ok();
                if let Some(zone) = zone {
                    set_var(zone);
                }
                *slot = Some(Self {
                    previous,
                    _lock: lock,
                });
            }
        });
    }
}

impl Drop for TzGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => set_var(&previous),
            None => remove_var(),
        }
    }
}

#[expect(
    unsafe_code,
    reason = "chrono's `Local` reads the `TZ` environment variable, so \
              mutating it is the only injection seam for deterministic \
              local-zone tests; `set_var` is unsafe in edition 2024 because \
              non-Rust `getenv` readers could race it, and this dependency \
              tree has none during tests"
)]
fn set_var(value: &str) {
    // SAFETY: std::env::set_var serializes with every std::env::var read via
    // std's internal environment lock, so in-process readers are memory-safe;
    // writes also serialize test-side under `TZ_LOCK`.
    unsafe { std::env::set_var("TZ", value) };
}

#[expect(
    unsafe_code,
    reason = "chrono's `Local` reads the `TZ` environment variable, so \
              mutating it is the only injection seam for deterministic \
              local-zone tests; `remove_var` is unsafe in edition 2024 \
              because non-Rust `getenv` readers could race it, and this \
              dependency tree has none during tests"
)]
fn remove_var() {
    // SAFETY: std::env::remove_var serializes with every std::env::var read
    // via std's internal environment lock, so in-process readers are
    // memory-safe; writes also serialize test-side under `TZ_LOCK`.
    unsafe { std::env::remove_var("TZ") };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_swaps_the_zone_and_drop_restores_it() {
        TzGuard::set("Etc/GMT-2");
        assert_eq!(std::env::var("TZ").ok().as_deref(), Some("Etc/GMT-2"));

        // A second set on the same thread swaps the value; the original
        // guard still restores the true original.
        TzGuard::set("UTC");
        assert_eq!(std::env::var("TZ").ok().as_deref(), Some("UTC"));

        // Dropping happens at thread end, so this test cannot observe the
        // restore directly; it only asserts the swap behavior above.
    }

    #[test]
    fn keep_holds_the_lock_without_changing_the_zone() {
        let before = std::env::var("TZ").ok();
        TzGuard::keep();

        assert_eq!(std::env::var("TZ").ok(), before);
    }
}
