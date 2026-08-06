//! Proves the trust → untrust → subsequent-command-fails round trip at the
//! process boundary — previously only unit-tested in-process.

use super::support::Sandbox;

#[test]
fn untrust_then_list_fails_with_the_untrusted_diagnostic() {
    let sandbox = Sandbox::trusted();
    let untrust = sandbox.run(&["untrust"]);
    assert!(untrust.is_success(), "stderr: {}", untrust.stderr);

    let list = sandbox.run(&["list"]);
    assert!(!list.is_success());
    assert!(
        list.stderr.contains("traces::cli::config_build_untrusted"),
        "stderr: {}",
        list.stderr
    );
}
