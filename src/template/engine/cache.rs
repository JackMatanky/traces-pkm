//! Cache render-scoped resources in `State`'s temp storage.
//!
//! [`cached`] backs `query.rs`'s [`FileIndex`](crate::index::FileIndex)
//! cache: it stashes a load result behind a fixed key so a render calling
//! into a namespace several times pays for one load, instead of
//! hand-rolling its own downcastable wrapper and get-or-load body per
//! resource. [`set_temp`]/[`get_temp`] are the same stash/retrieve
//! mechanism without a load step, for a value that's cheap to build (an
//! `Arc` clone) rather than expensive to load: `schema.rs` seeds the
//! render's [`SchemaService`](crate::schema::SchemaService) this way so
//! [`Schema`](crate::schema::Schema)'s own minijinja `Object` impl can reach
//! it without holding a reference itself.

use std::fmt;

use minijinja::{
    State,
    value::{Object, Value},
};

/// Wraps any render-scoped cacheable value so it can round-trip through
/// [`State`]'s temp storage via [`Value::from_object`]/
/// [`Value::downcast_object_ref`]. Never exposed to templates.
#[derive(Debug)]
struct Cached<T>(T);

impl<T: fmt::Debug + Send + Sync + 'static> Object for Cached<T> {}

/// Stashes `value` in `state`'s temp storage under `key`, for later retrieval
/// via [`get_temp`] within the same render. Overwrites any previous value
/// under `key`.
pub(super) fn set_temp<T>(state: &State, key: &'static str, value: T)
where
    T: Clone + fmt::Debug + Send + Sync + 'static,
{
    state.set_temp(key, Value::from_object(Cached(value)));
}

/// Returns the render-scoped value stashed under `key` via [`set_temp`], or
/// `None` if nothing was ever stashed there this render.
pub(super) fn get_temp<T>(state: &State, key: &'static str) -> Option<T>
where
    T: Clone + fmt::Debug + Send + Sync + 'static,
{
    state.get_temp(key).and_then(|value| {
        value.downcast_object_ref::<Cached<T>>().map(|c| c.0.clone())
    })
}

/// Returns the render-scoped value cached under `key`, loading it via `load`
/// and caching the result first if not already cached this render.
///
/// `T` should be cheap to clone (an `Arc<_>` or similarly shared type): every
/// call clones the cached value out of `state`'s temp storage rather than
/// handing back a reference tied to its lifetime.
///
/// # Errors
///
/// Any error `load` returns. Nothing is cached on failure, so the next call
/// retries the load.
pub(super) fn cached<T, E>(
    state: &State,
    key: &'static str,
    load: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    T: Clone + fmt::Debug + Send + Sync + 'static,
{
    if let Some(value) = get_temp(state, key) {
        return Ok(value);
    }
    let value = load()?;
    set_temp(state, key, value.clone());
    Ok(value)
}

#[cfg(test)]
mod tests {
    mod cached {
        use std::cell::Cell;

        use minijinja::Environment;
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn second_call_reuses_the_cached_value_without_reloading() {
            let env = Environment::new();
            let state = env.empty_state();
            let calls = Cell::new(0);
            let load = || {
                calls.set(calls.get() + 1);
                Ok::<_, ()>(42_i32)
            };

            let first = cached(&state, "test.cache_key", load);
            let second = cached(&state, "test.cache_key", load);

            assert_eq!(first, Ok(42));
            assert_eq!(second, Ok(42));
            assert_eq!(calls.get(), 1, "load must run once, not per call");
        }

        #[test]
        fn a_failed_load_is_not_cached_and_retries_next_call() {
            let env = Environment::new();
            let state = env.empty_state();
            let calls = Cell::new(0);
            let load = || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Err::<i32, _>("boom")
                } else {
                    Ok(7)
                }
            };

            let first = cached(&state, "test.retry_key", load);
            let second = cached(&state, "test.retry_key", load);

            assert_eq!(first, Err("boom"));
            assert_eq!(second, Ok(7));
            assert_eq!(calls.get(), 2, "a failed load must not be cached");
        }
    }

    mod temp {
        use minijinja::Environment;
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn get_temp_returns_none_before_any_set_temp_call() {
            let env = Environment::new();
            let state = env.empty_state();

            assert_eq!(get_temp::<i32>(&state, "test.unset_key"), None);
        }

        #[test]
        fn get_temp_returns_the_value_set_temp_stashed() {
            let env = Environment::new();
            let state = env.empty_state();

            set_temp(&state, "test.stash_key", 99_i32);

            assert_eq!(get_temp::<i32>(&state, "test.stash_key"), Some(99));
        }

        #[test]
        fn set_temp_overwrites_a_previously_stashed_value() {
            let env = Environment::new();
            let state = env.empty_state();

            set_temp(&state, "test.overwrite_key", 1_i32);
            set_temp(&state, "test.overwrite_key", 2_i32);

            assert_eq!(get_temp::<i32>(&state, "test.overwrite_key"), Some(2));
        }
    }
}
