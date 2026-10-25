use std::ffi::OsStr;

use super::Endpoint;

#[test]
fn endpoint_cache_keys_are_normalised() {
    let key =
        |s: &str| Endpoint::from_env(Some(OsStr::new(s))).unwrap().cache_key();
    assert_eq!(key("HTTPS://Sourcegraph.COM/"), "https://sourcegraph.com");
    assert_eq!(key("https://user:pw@host:3080/sg/"), "https://host:3080/sg");
    assert_ne!(key("http://x"), key("https://x"));
    assert_ne!(key("https://x/a"), key("https://x"));
}
