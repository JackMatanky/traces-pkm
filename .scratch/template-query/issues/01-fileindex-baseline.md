# 01 — FileIndex Baseline

**What to build:** A trusted project can run `traces index` and persist a FileIndex containing File Records for all files. The FileIndex can be loaded again and uses `created_at`, `modified_at`, and `size` as observable freshness metadata.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] `traces index` builds a persisted FileIndex for a trusted project root.
- [ ] Every indexed file has a File Record with path, name, folder, created time, modified time, and size.
- [ ] The persisted FileIndex can be loaded in a later command invocation.
- [ ] Freshness metadata is stored in terms of `created_at`, `modified_at`, and `size`.
- [ ] Tests cover the FileIndex through its public interface and the CLI through command dispatch.
