# Mocks

> Prefer a small fake behind a trait; use `mockall` when expectations matter.

## Trait Boundary

Put external dependencies behind traits so tests can inject fakes for databases, HTTP APIs, filesystems, clocks, and failure modes that are hard to trigger against the real dependency.

Do not mock pure deterministic functions. Do not mock the real dependency when the integration itself is what needs testing.

Concrete dependencies make error paths, timeouts, and edge cases hard to cover without slow or flaky external systems.

```rust
trait UserRepository {
    fn find_by_id(&self, id: u64) -> Result<Option<User>, DbError>;
}

struct UserService<R: UserRepository> {
    repo: R,
}
```

Use `Box<dyn Trait>` when generics would leak too far through the API.

For async dependencies, the trait usually needs `Send + Sync`, and projects commonly use `async_trait` if native async traits are not enough for the chosen mock approach.

## Hand-Written Fakes

For simple behavior, a fake is clearer than a mocking framework.

```rust
struct FakeUserRepo {
    users: HashMap<u64, User>,
}

impl UserRepository for FakeUserRepo {
    fn find_by_id(&self, id: u64) -> Result<Option<User>, DbError> {
        Ok(self.users.get(&id).cloned())
    }
}
```

## `mockall`

Use `mockall` for call counts, ordered calls, argument matching, many trait methods, or generated async trait mocks.

```rust
use mockall::automock;

#[automock]
trait Database {
    fn get_user(&self, id: u64) -> Option<User>;
}

#[test]
fn find_user_returns_name_from_repo() {
    let mut mock = MockDatabase::new();
    mock.expect_get_user()
        .with(mockall::predicate::eq(42))
        .times(1)
        .returning(|_| Some(User { id: 42, name: "Alice".into() }));

    assert_eq!(UserService::new(mock).find_user(42).unwrap().name, "Alice");
}
```

Expectations are verified when the mock drops.

Useful expectation tools: `.times(...)` for call counts, `.with(...)` or `.withf(...)` for argument predicates, `Sequence` for ordered calls, and `.returning(...)` for input-dependent values.

## Choice

| Situation | Prefer |
|---|---|
| One or two methods, simple return values | Hand-written fake |
| Call count or sequence matters | `mockall` |
| Many trait methods, only some used per test | `mockall` |
| One forced error path | Fake or `mockall`, whichever is shorter |
| Async trait with expectations | `mockall` plus the project's async-trait pattern |
