//! Proves the config trust → untrust lifecycle through `ConfigService` alone,
//! independent of any CLI command.
//!
//! `ConfigService::load` stays `pub(crate)`: its error type (`ConfigLoadError`)
//! transitively wraps several typestate-internal config types
//! (`DiscoveryScope`, `ConfigTrustStatus`, `LocalConfigFile<Tracked>`, …) that
//! exist purely to enforce the crate's own load pipeline invariants and carry
//! no value as public API. Proving the round trip through
//! `create_trusted_project`'s success (trust) and `untrust`'s returned count
//! exercises the same observable contract without that unwarranted surface
//! widening.

use pretty_assertions::assert_eq;
use traces_pkm::{TrustRequest, create_trusted_project, fixture_service};

/// Trusts then untrusts a project via `ConfigService`, checking the
/// removed-entry count.
///
/// `src/config/service.rs` unit-tests the same trust/untrust logic from
/// inside the crate. This is the only test proving that logic still works
/// through the `pub` surface real consumers get — it would stay green if
/// only the public re-export broke.
#[test]
fn trust_then_untrust_round_trips_through_the_public_service_surface() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    let service = fixture_service(temp.path());

    // `create_trusted_project` panics internally if `service.trust(...)`
    // fails, so returning normally already proves the trust half of the
    // round trip.
    let config_path = create_trusted_project(&service, &root);
    assert!(config_path.is_file());

    let removed = service
        .untrust(&TrustRequest::from(root.as_path()))
        .expect("untrust project root");
    assert_eq!(removed, 1);
}
