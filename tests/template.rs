//! Service-file templates: the `{{placeholder}}`s, what may go into one, and
//! the home-folder rule. (Moved from spi's inline tests.)

use air_minterface::template::{expand, plain, under_home};

fn vars() -> Vec<(&'static str, String)> {
    vec![("a", "1 & 2".into()), ("b_xml", "<x/>".into())]
}

#[test]
fn placeholders_are_replaced_and_text_around_them_is_left_alone() {
    let out = expand("t", "x {{a}} y {{ a }} ${z} {not}", &vars(), false).unwrap();
    assert_eq!(out, "x 1 & 2 y 1 & 2 ${z} {not}");
}

#[test]
fn xml_values_are_escaped_except_the_ones_that_say_they_are_xml() {
    let out = expand("t", "<s>{{a}}</s>{{b_xml}}", &vars(), true).unwrap();
    assert_eq!(out, "<s>1 &amp; 2</s><x/>");
}

#[test]
fn an_unknown_or_unclosed_placeholder_is_an_error_that_names_it() {
    let e = expand("the unit", "{{nope}}", &vars(), false).unwrap_err();
    assert!(e.contains("the unit") && e.contains("{{nope}}") && e.contains("a, b_xml"));
    let e = expand("the unit", "x {{a", &vars(), false).unwrap_err();
    assert!(e.contains("never closed"));
}

#[test]
fn only_plain_values_go_into_a_unit() {
    assert!(plain("p", "/usr/local/bin").is_ok());
    assert!(plain("p", "-datadir=/var/lib/x_y-1.2").is_ok());
    for bad in [
        "", "/a b", "/a'b", "/a\"b", "/a%b", "/a$b", "/a\\b", "/a\nb", "/a;b",
    ] {
        let e = plain("p", bad).unwrap_err();
        assert!(
            e.starts_with(&format!("p is `{bad}`: in a service file it may hold only")),
            "{bad:?}: {e}"
        );
    }
}

#[test]
fn home_folders_are_found() {
    for p in [
        "/home/x",
        "/home/x/y",
        "/root",
        "/root/x",
        "/Users/x/Library",
    ] {
        assert!(under_home(p), "{p}");
    }
    for p in ["/var/lib/sp-mint", "/homework", "/usr/local/bin", "/Users"] {
        assert!(!under_home(p), "{p}");
    }
}
