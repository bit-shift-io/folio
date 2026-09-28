//! Assets embedded into the binary by `build.rs`.
//!
//! One flat table keyed by forward-slash project-relative path: the frontend
//! under `web/dist/` and the icon theme under `res/icons/`. Icons are *also*
//! read from disk at runtime (`res/icons` is a plain directory, so swapping a
//! theme needs no rebuild); the embedded copy is the fallback for release
//! builds launched outside the project directory.

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// Looks up an embedded asset by its table key.
pub fn get(key: &str) -> Option<&'static [u8]> {
    ASSETS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, bytes)| *bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(prefix: &str) -> usize {
        ASSETS.iter().filter(|(k, _)| k.starts_with(prefix)).count()
    }

    #[test]
    fn table_contains_the_frontend() {
        for key in [
            "web/dist/index.html",
            "web/dist/app.js",
            "web/dist/style.css",
        ] {
            assert!(get(key).is_some_and(|b| !b.is_empty()), "missing {key}");
        }
    }

    #[test]
    fn table_contains_the_icon_theme() {
        assert!(
            has("res/icons/") >= 49,
            "expected the whole vendored theme, got {}",
            has("res/icons/")
        );
        assert!(get("res/icons/breeze-dark/places/96/folder.svg").is_some());
    }

    #[test]
    fn keys_are_forward_slash_and_relative() {
        for (key, _) in ASSETS {
            assert!(!key.contains('\\'), "key must be forward-slash: {key}");
            assert!(
                key.starts_with("web/dist/") || key.starts_with("res/icons/"),
                "unexpected key: {key}"
            );
        }
    }

    #[test]
    fn get_returns_none_for_a_miss() {
        assert!(get("web/dist/nope.js").is_none());
        assert!(get("").is_none());
    }
}
