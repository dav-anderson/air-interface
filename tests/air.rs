//! What both operators know about the AIR server: its health answer.

use air_interface::air;

#[test]
fn only_a_bare_ok_with_status_200_is_a_healthy_air_server() {
    // Fails if a proxy's 200 page, or a body that merely contains `ok`, passes
    // for air_server (the operators used to disagree on this).
    assert!(air::healthy(200, "ok"));
    assert!(air::healthy(200, "ok\n"));
    assert!(!air::healthy(200, "<html>ok</html>"));
    assert!(!air::healthy(200, ""));
    assert!(!air::healthy(503, "ok"));
}
