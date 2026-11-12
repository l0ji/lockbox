//! Tests of the canonical URL validation function.

use lockbox_core::canonical::canonical;

#[test]
fn rewrites_a_link_into_the_form_which_decides_identity() {
    assert_eq!(
        canonical("http://www.Example.com:80/A/?utm_source=feed&b=2&a=1#top").expect("canonical"),
        "http://example.com/A?a=1&b=2"
    );
    assert_eq!(
        canonical("https://example.com/a/").expect("canonical"),
        "https://example.com/a"
    );
    assert_eq!(
        canonical("https://example.com").expect("canonical"),
        "https://example.com/"
    );
    assert_eq!(
        canonical("https://example.com/?q=1").expect("canonical"),
        "https://example.com/?q=1"
    );
}

#[test]
fn drops_every_tracking_parameter() {
    let canonical = canonical(
        "https://example.com/a?utm_campaign=x&utm_medium=y&fbclid=z&gclid=w&mc_cid=v&ref_src=u",
    )
    .expect("canonical");
    assert_eq!(canonical, "https://example.com/a");
}

#[test]
fn keeps_what_is_not_tracking() {
    assert_eq!(
        canonical("https://example.com/search?q=rust&page=2").expect("canonical"),
        "https://example.com/search?page=2&q=rust"
    );
}

#[test]
fn refuses_invalid_links() {
    for text in ["", "not a link"] {
        assert!(canonical(text).is_err(), "{text}");
    }
}

#[test]
fn accepts_document_schemes_without_web_specific_rewrites() {
    for (input, expected) in [
        (
            "gemini://example.com/a/?utm_source=x#part",
            "gemini://example.com/a/?utm_source=x#part",
        ),
        (
            "gopher://example.com/1selector%09search",
            "gopher://example.com/1selector%09search",
        ),
    ] {
        assert_eq!(canonical(input).expect("canonical"), expected);
    }
}

#[test]
fn refuses_unsupported_schemes_and_invalid_authorities() {
    for text in [
        "ftp://example.com/a",
        "mailto:jane@example.com",
        "javascript:alert(1)",
        "custom:some/path",
        "gemini:/path",
        "gopher:///path",
        "gemini://user@example.com/",
        "gopher://user:pass@example.com/",
    ] {
        assert!(canonical(text).is_err(), "{text}");
    }
}
