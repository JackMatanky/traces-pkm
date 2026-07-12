# Fixtures and Cleanup

> Use RAII (the `Drop` trait) so cleanup runs even when the test panics.

## Why It Matters

Tests that create temp files, set env vars, or start servers need teardown — and a manual teardown line at the end of the test function *does not run* if an earlier assertion panics. That leaks state into later tests (a leftover file, a stuck env var) and produces confusing, order-dependent failures. RAII ties cleanup to a value's lifetime via `Drop`, so it runs during unwinding too.

## When to Use

Any time a test acquires a resource that must be released regardless of whether the test passes, fails, or panics midway: temp files/dirs, environment variables, background threads/servers, database transactions.

## Bad

```rust
#[test]
fn test_with_temp_file() {
    let path = "/tmp/test_file.txt";
    std::fs::write(path, "test data").unwrap();

    let result = process_file(path);

    std::fs::remove_file(path).unwrap(); // never runs if the assert below panics
    assert!(result.is_ok());
}
```

## Good: `tempfile` for Files and Directories

```toml
[dev-dependencies]
tempfile = "3"
```

```rust
use tempfile::{NamedTempFile, TempDir};

#[test]
fn process_file_returns_ok_for_valid_input() {
    // Arrange — file is deleted automatically when `file` drops, panic or not
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "test data").unwrap();

    // Act
    let result = process_file(file.path());

    // Assert — cleanup happens even if this panics
    assert!(result.is_ok());
}

#[test]
fn process_dir_walks_all_entries() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("test.txt"), "data").unwrap();

    // dir and everything inside it is deleted on drop
}
```

## Good: A Custom RAII Guard for Environment Variables

```rust
struct EnvGuard {
    key: String,
    original: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        // SAFETY: env::set_var is unsafe since the 2024 edition (env writes
        // aren't thread-safe); env-touching tests must run single-threaded
        // (`cargo test -- --test-threads=1` for this module, or serial_test).
        unsafe { std::env::set_var(key, value) };
        EnvGuard { key: key.to_string(), original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: see EnvGuard::set — restored on the same single-threaded test
        match &self.original {
            Some(v) => unsafe { std::env::set_var(&self.key, v) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}

#[test]
fn read_config_uses_env_override() {
    let _guard = EnvGuard::set("MY_VAR", "test_value");

    let result = read_config();

    assert!(result.is_ok());
} // MY_VAR restored here, even on panic
```

## Other RAII Patterns

```rust
// Server that shuts down on drop
struct TestServer {
    handle: std::thread::JoinHandle<()>,
    shutdown: std::sync::mpsc::Sender<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
    }
}

// Database transaction that always rolls back
struct TestTransaction<'a> {
    conn: &'a mut Connection,
}

impl Drop for TestTransaction<'_> {
    fn drop(&mut self) {
        self.conn.execute("ROLLBACK").unwrap();
    }
}
```

## `scopeguard` for One-Off Cleanup

When a full guard struct is overkill, `scopeguard::defer!` runs a closure on scope exit without defining a type:

```toml
[dev-dependencies]
scopeguard = "1"
```

```rust
use scopeguard::defer;

#[test]
fn process_file_cleans_up_temp_output() {
    let path = "/tmp/test_output.txt";
    std::fs::write(path, "data").unwrap();

    defer! {
        std::fs::remove_file(path).ok();
    }

    // test logic — cleanup runs on scope exit even if this panics
}
```

## Anti-Patterns

| Anti-Pattern | Fix |
|---|---|
| Cleanup as the last line of the test body | RAII guard (`Drop`) — runs during unwinding, manual cleanup doesn't |
| Sharing one temp path/dir string across tests | A fresh `tempfile`/`TempDir` per test |
| Mutating global env vars without restoring | `EnvGuard` pattern above, and keep those tests single-threaded |

## See Also

- [`unit-testing.md`](unit-testing.md) — fixture guidance in general (keep fixtures local, name them concretely)
- [`async-testing.md`](async-testing.md) — cleanup patterns for async tests specifically
