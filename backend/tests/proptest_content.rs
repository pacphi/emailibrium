//! Property-based tests for content extraction (R-10).
//!
//! Requires `proptest` in dev-dependencies.
//! Add to Cargo.toml [dev-dependencies]: proptest = "1.8"

#[cfg(feature = "proptest")]
mod content_props {
    use proptest::prelude::*;

    use emailibrium::content::html_extractor::HtmlExtractor;

    proptest! {
        /// Arbitrary HTML should never cause a panic in content extraction.
        #[test]
        fn content_extraction_never_panics(html in ".*") {
            let _ = HtmlExtractor::extract_text(&html);
        }

        /// Extracted text from well-formed HTML tags should not contain raw tags.
        #[test]
        fn extracted_text_has_no_raw_tags(
            tag in "[a-z]{1,8}",
            content in "[^<>]{0,100}",
        ) {
            let html = format!("<{tag}>{content}</{tag}>");
            let result = HtmlExtractor::extract_text(&html);
            // The extracted text should not contain the opening angle bracket
            // followed by a tag name (i.e., no raw HTML tags remain).
            prop_assert!(
                !result.contains(&format!("<{tag}>")),
                "Extracted text still contains <{}>: {}",
                tag,
                result
            );
        }

        /// Link extraction should never panic on arbitrary HTML.
        #[test]
        fn link_extraction_never_panics(html in ".*") {
            let _ = HtmlExtractor::extract_links(&html);
        }

        /// Image extraction should never panic on arbitrary HTML.
        #[test]
        fn image_extraction_never_panics(html in ".*") {
            let _ = HtmlExtractor::extract_images(&html);
        }

        /// Extracted text length should never exceed input length.
        #[test]
        fn extracted_text_not_longer_than_input(html in ".{0,500}") {
            let result = HtmlExtractor::extract_text(&html);
            // Extracted plain text should be at most as long as the original HTML
            // (tags are stripped, not added).
            prop_assert!(
                result.len() <= html.len() + 1, // +1 for possible trailing newline
                "Extracted text ({}) longer than input ({})",
                result.len(),
                html.len()
            );
        }
    }
}

/// Ensure the test file compiles even without the proptest feature.
#[test]
fn proptest_content_placeholder() {
    // This test exists so `cargo test` finds at least one test in this file.
    // The real property tests require: cargo test --features proptest
    assert!(true);
}
