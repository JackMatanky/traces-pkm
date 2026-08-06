//! End-to-end test harness: spawns the real `traces` binary (or, for `init`'s
//! inherently-interactive flow, drives it in-process) against fully isolated
//! sandboxes. See `support` for isolation guarantees.
#![expect(
    clippy::expect_used,
    reason = "this whole binary is test fixture/harness code; a failed \
              .expect() here means the sandbox itself is broken and should \
              panic the test immediately"
)]

#[path = "e2e/support.rs"]
mod support;

#[path = "e2e/dispatch.rs"]
mod dispatch;
#[path = "e2e/golden_path.rs"]
mod golden_path;
#[path = "e2e/init.rs"]
mod init;
#[path = "e2e/template_write.rs"]
mod template_write;
#[path = "e2e/tracked.rs"]
mod tracked;
#[path = "e2e/untrust.rs"]
mod untrust;
