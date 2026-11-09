//! The router suite of `T-004-02` (`DEC-5`), run on the library.

use rutent_conformance::router::run;
use rutent_router::RouteFinder;

// verifies: AC-2, REQ-AUD-4, REQ-QTE-11, DEC-5
#[test]
fn the_router_suite_passes_on_the_library() {
    let report = run::<RouteFinder>();
    assert!(report.passes(), "{}", report.summary());
}
