# RAII Cleanup

> Use RAII (`Drop`) when test setup must be cleaned up even if the test panics.

## When to Use

Use a guard for temp files/directories, environment variables, server handles, database transactions, or any resource that would pollute later tests if cleanup is skipped.

Do not write manual teardown after the assertion; it will not run if the assertion panics.

## Temp Files

Use `Drop`, `tempfile`, or `scopeguard` when cleanup must run on panic.

```rust
#[test]
fn process_file_returns_ok_for_valid_input() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "test data").unwrap();

    let result = process_file(file.path());

    assert!(result.is_ok());
} // file is removed even if the assertion panics
```

## Environment Variables

For global state such as environment variables, use a named guard that restores the original value in `Drop`, and run those tests single-threaded.

```rust
struct EnvGuard {
    key: String,
    original: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        // SAFETY: environment writes are process-global; run these tests single-threaded.
        unsafe { std::env::set_var(key, value) };
        Self { key: key.to_string(), original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: restored under the same single-threaded test constraint.
        match &self.original {
            Some(value) => unsafe { std::env::set_var(&self.key, value) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}

#[test]
fn reads_config_from_env() {
    let _guard = EnvGuard::set("APP_MODE", "test");
    assert_eq!(read_mode(), "test");
}
```

## Common Guards

| Resource | Guard |
|---|---|
| Temp file | `tempfile::NamedTempFile` |
| Temp directory | `tempfile::TempDir` |
| One-off cleanup | `scopeguard::defer!` |
| Env var | Custom `Drop` guard plus single-threaded execution |
| Server/thread | Guard that sends shutdown in `Drop` |
| DB transaction | Guard that rolls back in `Drop` |

## Completion

Every fixture either builds Arrange data only or is explicitly named as a cleanup guard; hidden assertions in helpers belong in `rust-unit-testing/references/code-quality.md` findings.
