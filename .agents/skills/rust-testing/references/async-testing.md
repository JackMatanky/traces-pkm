# Async Testing

> Use `#[tokio::test]` to run async test functions — an `async fn` can't be called directly without a runtime.

## Why It Matters

`async fn` bodies don't execute until polled by a runtime. `#[tokio::test]` sets up a Tokio runtime for the test and drives the `async fn` to completion automatically, replacing the boilerplate of manually building a `Runtime` and calling `block_on`.

## Bad

```rust
// Won't compile — #[test] can't call an async fn
#[test]
async fn test_async_function() {
    let result = fetch_data().await;
    assert!(result.is_ok());
}

// Compiles, but verbose and easy to get wrong
#[test]
fn test_async_function() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = fetch_data().await;
        assert!(result.is_ok());
    });
}
```

## Good

```rust
#[tokio::test]
async fn fetch_data_returns_ok_for_valid_request() {
    let result = fetch_data().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn fetches_two_users_concurrently() {
    let (a, b) = tokio::join!(fetch_user(1), fetch_user(2));
    assert!(a.is_ok());
    assert!(b.is_ok());
}
```

## Runtime Configuration

```rust
// Default: multi-threaded runtime
#[tokio::test]
async fn uses_default_runtime() {}

// Single-threaded — simpler, deterministic ordering
#[tokio::test(flavor = "current_thread")]
async fn uses_single_threaded_runtime() {}

// Explicit worker count
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uses_two_worker_threads() {}

// Paused virtual time — for deterministic timeout/interval tests
#[tokio::test(start_paused = true)]
async fn advances_time_deterministically() {
    tokio::time::advance(Duration::from_secs(60)).await;
}
```

Prefer `current_thread` for tests that don't need real parallelism — it removes a source of nondeterministic interleaving. Prefer `start_paused = true` over real `sleep`s whenever a test needs to assert something about elapsed time or timeout behavior — it makes the test both deterministic and fast.

## Testing Timeouts

```rust
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn slow_operation_completes_within_five_seconds() {
    let result = timeout(Duration::from_secs(5), slow_operation()).await;
    assert!(result.is_ok(), "operation timed out");
}

#[tokio::test]
async fn never_completes_triggers_timeout() {
    let result = timeout(Duration::from_millis(100), never_completes()).await;
    assert!(result.is_err(), "expected a timeout");
}
```

## Testing Channels

```rust
use tokio::sync::mpsc;

#[tokio::test]
async fn channel_delivers_messages_in_order() {
    let (tx, mut rx) = mpsc::channel(10);

    tokio::spawn(async move {
        tx.send("hello").await.unwrap();
        tx.send("world").await.unwrap();
    });

    assert_eq!(rx.recv().await, Some("hello"));
    assert_eq!(rx.recv().await, Some("world"));
    assert_eq!(rx.recv().await, None);
}
```

## Combining with Mocks

```rust
#[tokio::test]
async fn find_user_returns_name_from_mocked_database() {
    let mut mock = MockDatabase::new();
    mock.expect_get_user()
        .with(eq(42))
        .returning(|_| Some(User { id: 42, name: "Alice".into() }));

    let service = UserService::new(mock);

    assert_eq!(service.find_user(42).await.unwrap().name, "Alice");
}
```

See [`mocking.md`](mocking.md) for trait/mock design.

## See Also

- [`unit-testing.md`](unit-testing.md) — AAA structure applies the same way to async tests
- [`mocking.md`](mocking.md) — mocking async trait dependencies
- [`fixtures-and-cleanup.md`](fixtures-and-cleanup.md) — RAII cleanup for async resources (servers, connections)
- [`concurrency-testing.md`](concurrency-testing.md) — proving correctness of the primitives underneath async code, not just testing the async fn's behavior
