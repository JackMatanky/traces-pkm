//! Cache one render-scoped resource load in `State`'s temp storage.
//!
//! [`cached`] backs `query.rs`'s [`FileIndex`](crate::index::FileIndex) cache
//! and the [`SchemaRegistry`](crate::schema::SchemaRegistry) cache shared by
//! the `query`, `tasks`, and `schema` namespaces: each stashes a load result
//! behind a fixed key so a render calling into a namespace several times pays
//! for one load, instead of hand-rolling its own downcastable wrapper and
//! get-or-load body per resource.

use std::fmt;

use minijinja::{
    State,
    value::{Object, Value},
};

/// The [`State::set_temp`] key caching one loaded
/// [`SchemaRegistry`](crate::schema::SchemaRegistry) for the current render.
///
/// Shared by the `query`, `tasks`, and `schema` namespaces. This is sound only
/// because [`super::TemplateEngine::new`] builds one
/// [`SchemaContext`](super::schema::SchemaContext) and clones its `Arc` into
/// every namespace that needs the Schema registry directory, instead of each
/// namespace independently constructing its own `Arc<Path>` that merely
/// happens to name the same directory today. A render touching both a
/// `from_class` query and `schema.get` pays for one
/// [`SchemaRegistry::load`](crate::schema::SchemaRegistry::load), not one per
/// namespace.
pub(super) const SCHEMA_REGISTRY_CACHE_KEY: &str = "schema.registry_cache";

/// Wraps any render-scoped cacheable value so it can round-trip through
/// [`State`]'s temp storage via [`Value::from_object`]/
/// [`Value::downcast_object_ref`]. Never exposed to templates.
#[derive(Debug)]
struct Cached<T>(T);

impl<T: fmt::Debug + Send + Sync + 'static> Object for Cached<T> {}

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
    if let Some(value) = state.get_temp(key).and_then(|value| {
        value.downcast_object_ref::<Cached<T>>().map(|cached| cached.0.clone())
    }) {
        return Ok(value);
    }
    let value = load()?;
    state.set_temp(key, Value::from_object(Cached(value.clone())));
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use minijinja::Environment;
    use pretty_assertions::assert_eq;

    use super::*;

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
