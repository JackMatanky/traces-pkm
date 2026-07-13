# Trust store: check + create + untrusted rejection

Status: ready-for-agent

## Parent

`.scratch/config-service/spec.md`

## What to build

The trust mechanism, following the mise `config/tracking.rs` pattern (see ADR 0002, now accepted). A directory is trusted by creating a symlink (plain file on Windows) in the **trusted** store, named by the SHA-256 hash of the canonical directory path, pointing back at the directory.

The shared symlink-store component (`ConfigFileStore` in `src/config/store.rs`) already exists — it is instance-based, holds its root as a field, and exposes `record`, `list_all`, and `clean`. The trust store path (`dirs::TRUSTED_CONFIGS`) is already defined in `src/config/dirs.rs` (currently `#[allow(dead_code)]`). Build a thin **`ConfigTrust` adapter** (analogous to `ConfigTracker` in `tracker.rs`) that wraps `ConfigFileStore` at `dirs::TRUSTED_CONFIGS`. `ConfigService` gains `trust(dir)` and `is_trusted(dir)`. An untrusted directory check yields a miette error that includes the path and suggests running `traces trust`.

The `Trusted` builder stage (`ConfigBuilder::trust()`, in `builder.rs`) is currently a no-op placeholder — issue 04 makes it real: `trust()` accepts `&ConfigTrust`, checks the parent directory of each candidate config file path via `ConfigTrust::is_trusted()`, and returns `Result<ConfigBuilder<Trusted>, ConfigError>`. An untrusted candidate directory fails the build with a miette error suggesting `traces trust`. This guards config loading at the source — if a config file lives in an untrusted directory, it's rejected before parsing.

This is the programmatic trust API; the `traces trust` CLI is issue 05.

## Current codebase state

Relevant existing code at a glance:

- **`ConfigFileStore`** (`src/config/store.rs`): shared component. `new(&dirs::StateDirRoot)` for production, `at(PathBuf)` for tests. Methods: `record(&self, target) -> Result`, `list_all(&self) -> Result<Vec<PathBuf>>`, `clean(&self) -> Result<usize>`. Cross-platform: `#[cfg(unix)]` symlink, `#[cfg(windows)]` plain file.
- **`dirs::TRUSTED_CONFIGS`** (`src/config/dirs.rs`): `LazyLock<StateDirRoot>`, resolves to `$TRACES_STATE_DIR/trusted-configs`. Currently `#[allow(dead_code)]`.
- **`ConfigTracker`** (`src/config/tracker.rs`): the pattern to follow. Wraps `ConfigFileStore`, has `new()` and `#[cfg(test)] at()`, non-`Result` `track()` that logs-and-swallows, `list_all()`, `clean()`.
- **`ConfigBuilder::trust()`** (`src/config/builder.rs:103`): no-op `Tracked → Trusted` transition.
- **`ConfigService`** (`src/config/service.rs`): has `tracker: ConfigTracker` field; expose a trust adapter the same way.
- **`dirs` crate**: removed. Path resolution is self-contained in `dirs.rs`.
- **ADR 0002**: accepted — trust store path (`trusted-configs` under state dir) is ratified.

## Acceptance criteria

- [ ] `trust(dir)` creates the hashed symlink/file in `dirs::TRUSTED_CONFIGS` via `ConfigFileStore::record`
- [ ] `is_trusted(dir)` returns true only when a valid trust entry exists for the canonical path
- [ ] Untrusted rejection error includes the path and a `traces trust` suggestion (miette)
- [ ] Canonicalization ensures the same directory hashes consistently regardless of relative path (handled by `ConfigFileStore::record` already — canonicalize-then-hash lives in the shared component)
- [ ] Trust logic reuses `ConfigFileStore` rather than reimplementing hashing/symlink/clean
- [ ] Tests verify trust creation, positive/negative checks, and rejection error — using temp dirs and `#[cfg(test)]` `ConfigFileStore::at`/`ConfigTrust::at`

## Rust guidance

Relevant skills: `m11-ecosystem`, `m06-error-handling`, `m13-domain-error`, `m01-ownership`.

- **Pattern-match `ConfigTracker`** (`src/config/tracker.rs:25-88`). Build a `ConfigTrust` with the same shape: a `store: ConfigFileStore` field, `new(&dirs::TRUSTED_CONFIGS)`, `#[cfg(test)] at(root: PathBuf)`, and domain methods.
- **`trust(dir)` delegates to `self.store.record(dir)`** — `ConfigFileStore::record` already canonicalizes then hashes (SHA-256 hex filename). Record returns `Result<(), StoreError>`; unlike tracking (best-effort), trust decisions should propagate errors since a security gate that silently fails is worse than a crash.
- **`is_trusted(dir)`** checks whether `dirs::TRUSTED_CONFIGS / hash(canonicalize(dir))` exists as a file. A simple `Path::exists` check is correct because `ConfigFileStore::record` created it as a symlink/file. No need to resolve the link target for the boolean check — only existence matters. Use `try_exists()` (`m01-ownership`) instead of `exists()` to distinguish "not found" from "I/O error" for the trust-error path.
- **Rejection error (`m13`):** the untrusted-directory error is **user-facing and actionable** — add a `ConfigError::Untrusted { path: PathBuf }` variant rendered through miette with a `help` note suggesting `traces trust`. Distinguish "not trusted" (expected, actionable) from "trust store I/O failed" (internal — wrap `StoreError`).
- **Enforcement in the builder:** `ConfigBuilder::trust()` takes `&ConfigTrust` and checks `candidate.path().parent()` for each config file. This is the programmatic gate — an untrusted config source directory blocks the build before `merge()` reads the file. The check iterates `self.local` then `self.global` so the error points at the first untrusted source.
- **New module:** create `src/config/trust.rs` for `ConfigTrust` and its error types.

## Unblocked by

Issue 03 (`config-tracking-store.md`) is implemented on `feat/config-tracking-store` — `ConfigFileStore` and `dirs::TRUSTED_CONFIGS` are available. Start immediately.
