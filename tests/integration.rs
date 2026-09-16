//! Integration test harness: proves cross-module data flow through
//! `traces_pkm`'s `test-utils`-gated public surface alone (no process spawning,
//! no crate-internal imports). See `tests/e2e.rs` for process-boundary coverage
//! of the same command surface.
#![cfg(feature = "test-utils")]
#![allow(
    clippy::expect_used,
    reason = "this whole binary is test fixture/harness code; a failed \
              .expect() here means the fixture itself is broken and should \
              panic the test immediately"
)]
#[path = "integration/config_lifecycle.rs"]
mod config_lifecycle;
#[path = "integration/index_persistence_roundtrip.rs"]
mod index_persistence_roundtrip;
#[path = "integration/index_query.rs"]
mod index_query;
#[path = "integration/schema_field_resolution.rs"]
mod schema_field_resolution;
#[path = "integration/task_tag_filters.rs"]
mod task_tag_filters;
#[path = "integration/template_render.rs"]
mod template_render;
