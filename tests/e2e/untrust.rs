//! Proves the trust → untrust → subsequent-command-fails round trip at the
//! process boundary — previously only unit-tested in-process.

use super::support::Sandbox;

/// Untrusts a trusted project, then checks a later `list` fails with the
/// untrusted diagnostic.
///
/// Needs two real process invocations: the contract is that untrust
/// survives past its own process and is honored on the next config load,
/// which a single-process unit test can't distinguish from a plain store
/// mutation.
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
