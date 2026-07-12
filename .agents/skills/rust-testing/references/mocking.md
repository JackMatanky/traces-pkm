# Mocking

> Extract dependencies behind a trait so tests can inject a fake, not a real database.

## Why It Matters

A struct that holds a concrete `PostgresConnection` can't be tested without a running Postgres — you can't cover timeouts, connection errors, or edge cases without a real (slow, flaky) external system. Extracting the dependency behind a trait lets tests inject a test double instead, so unit tests stay fast, isolated, and able to cover error paths that would be painful to trigger against the real thing.

## When to Use

Reach for a trait + mock when the code under test calls out to a database, HTTP API, filesystem, clock, or any other dependency whose failure modes you need to test but can't reliably trigger against the real implementation.

Do **not** reach for this when:
- The dependency is a pure, deterministic function — just call it directly.
- You're testing the *integration* with the real dependency itself (e.g. does this SQL actually work against Postgres) — that belongs in an integration test with a real or containerized dependency, not a mock.

## Design: Depend on a Trait, Not a Concrete Type

```rust
// Bad: concrete type, can't test without a real database
struct UserService {
    db: PostgresConnection,
}

impl UserService {
    async fn get_user(&self, id: u64) -> Result<User, Error> {
        self.db.query("SELECT * FROM users WHERE id = $1", &[&id]).await
    }
}

#[tokio::test]
async fn test_get_user() {
    let db = PostgresConnection::connect("postgres://...").await?; // slow, flaky
    // ...
}
```

```rust
// Good: trait boundary
#[async_trait]
trait UserRepository: Send + Sync {
    async fn find_by_id(&self, id: u64) -> Result<Option<User>, DbError>;
    async fn save(&self, user: &User) -> Result<(), DbError>;
}

struct PostgresUserRepo {
    pool: PgPool,
}

#[async_trait]
impl UserRepository for PostgresUserRepo {
    async fn find_by_id(&self, id: u64) -> Result<Option<User>, DbError> {
        sqlx::query_as("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }
    // ...
}

struct UserService<R: UserRepository> {
    repo: R,
}

impl<R: UserRepository> UserService<R> {
    async fn get_user(&self, id: u64) -> Result<User, Error> {
        self.repo.find_by_id(id).await?.ok_or(Error::NotFound)
    }
}
```

Use `Box<dyn UserRepository>` instead of the generic parameter when you don't want generics to propagate through the rest of the API — slight runtime cost, cleaner call sites:

```rust
struct UserService {
    repo: Box<dyn UserRepository>,
}

impl UserService {
    fn new(repo: impl UserRepository + 'static) -> Self {
        Self { repo: Box::new(repo) }
    }
}
```

## Hand-Written Fakes

For simple cases, a hand-written struct implementing the trait is often clearer than a mocking framework:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct FakeUserRepo {
        users: HashMap<u64, User>,
    }

    #[async_trait]
    impl UserRepository for FakeUserRepo {
        async fn find_by_id(&self, id: u64) -> Result<Option<User>, DbError> {
            Ok(self.users.get(&id).cloned())
        }
        // ...
    }

    #[tokio::test]
    async fn get_user_returns_user_when_found() {
        let mut users = HashMap::new();
        users.insert(1, User { id: 1, name: "Alice".into() });
        let service = UserService { repo: FakeUserRepo { users } };

        let user = service.get_user(1).await.unwrap();

        assert_eq!(user.name, "Alice");
    }

    #[tokio::test]
    async fn get_user_returns_not_found_when_missing() {
        let service = UserService { repo: FakeUserRepo { users: HashMap::new() } };

        let result = service.get_user(999).await;

        assert!(matches!(result, Err(Error::NotFound)));
    }
}
```

A fake that always fails is useful for testing error paths specifically:

```rust
struct FailingClient;

#[async_trait]
impl HttpClient for FailingClient {
    async fn get(&self, _url: &str) -> Result<Response, HttpError> {
        Err(HttpError::Timeout)
    }
}

#[tokio::test]
async fn fetch_data_wraps_timeout_as_network_error() {
    let service = ApiService { client: FailingClient };

    let result = service.fetch_data().await;

    assert!(matches!(result, Err(Error::NetworkError(_))));
}
```

## `mockall` for Generated Mocks

When you need call-count verification, argument matching, or many trait methods, generating the mock with `mockall` beats hand-writing one:

```toml
[dev-dependencies]
mockall = "0.13"
```

```rust
use mockall::automock;

#[automock]
trait Database {
    fn get_user(&self, id: u64) -> Option<User>;
    fn save_user(&self, user: &User) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;

    #[test]
    fn find_user_returns_name_from_repo() {
        let mut mock = MockDatabase::new();
        mock.expect_get_user()
            .with(eq(42))
            .returning(|_| Some(User { id: 42, name: "Alice".into() }));

        let service = UserService::new(mock);

        assert_eq!(service.find_user(42).unwrap().name, "Alice");
    }
}
```

### Expectations and Call Counts

```rust
mock.expect_save_user()
    .times(1)                 // exactly once
    .returning(|_| Ok(()));

mock.expect_get_user()
    .times(3..)                // at least 3 times
    .returning(|_| None);
// Expectations are verified when the mock drops — an unmet expectation fails the test.
```

### Argument Matching

```rust
use mockall::predicate::*;

mock.expect_process()
    .with(eq(42))                            // exact match
    .returning(|_| Ok(()));

mock.expect_validate()
    .with(function(|s: &str| s.len() > 5))   // custom predicate
    .returning(|_| true);

mock.expect_search()
    .withf(|query, limit| query.len() < 100 && *limit <= 1000) // multiple args
    .returning(|_, _| vec![]);
```

### Ordered Calls

```rust
use mockall::Sequence;

let mut seq = Sequence::new();
mock.expect_connect().times(1).in_sequence(&mut seq).returning(|| Ok(()));
mock.expect_query().times(1).in_sequence(&mut seq).returning(|_| Ok(vec![]));
mock.expect_disconnect().times(1).in_sequence(&mut seq).returning(|| Ok(()));
```

### Async Traits

```rust
#[automock]
#[async_trait]
trait AsyncDatabase {
    async fn fetch(&self, id: u64) -> Option<Data>;
}

#[tokio::test]
async fn fetch_returns_data() {
    let mut mock = MockAsyncDatabase::new();
    mock.expect_fetch().returning(|_| Some(Data::default()));

    assert!(mock.fetch(1).await.is_some());
}
```

### Mocking a Trait You Don't Own

```rust
#[cfg_attr(test, mockall::automock)]
trait HttpClient {
    fn get(&self, url: &str) -> Result<Response, Error>;
}
```

## Hand-Written Fake vs `mockall`

| Situation | Prefer |
|---|---|
| One or two methods, simple return values | Hand-written fake — no macro magic, easy to read |
| Need call-count/sequence verification | `mockall` |
| Many trait methods, only a few used per test | `mockall` — avoids implementing every method by hand each time |
| Testing a specific error path | Either — a hand-written `FailingClient`-style fake is often clearest |

## See Also

- [`unit-testing.md`](unit-testing.md) — where mock setup fits in Arrange
- [`async-testing.md`](async-testing.md) — `#[tokio::test]` for async trait mocks
