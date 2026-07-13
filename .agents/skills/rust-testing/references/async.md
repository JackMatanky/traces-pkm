# Async Testing

> Use `#[tokio::test]` to run async test functions; an `async fn` needs a runtime to be polled.

## Runtime

```rust
#[tokio::test]
async fn fetch_data_returns_ok_for_valid_request() {
    let result = fetch_data().await;
    assert!(result.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn uses_single_threaded_runtime() {}

#[tokio::test(start_paused = true)]
async fn advances_time_deterministically() {
    tokio::time::advance(Duration::from_secs(60)).await;
}
```

Prefer `current_thread` when real parallelism is not under test. Prefer `start_paused = true` over real sleeps for timeout or interval behavior.

## Timeouts

```rust
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn slow_operation_completes_within_five_seconds() {
    let result = timeout(Duration::from_secs(5), slow_operation()).await;
    assert!(result.is_ok(), "operation timed out");
}
```

## Channels

```rust
#[tokio::test]
async fn channel_delivers_messages_in_order() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);

    tokio::spawn(async move {
        tx.send("hello").await.unwrap();
        tx.send("world").await.unwrap();
    });

    assert_eq!(rx.recv().await, Some("hello"));
    assert_eq!(rx.recv().await, Some("world"));
    assert_eq!(rx.recv().await, None);
}
```

Use `mocks.md` for async trait seams and `raii-cleanup.md` for cleanup guards around async resources.
