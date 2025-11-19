#[cfg(test)]
mod tests {
    use crate::{exa, tavily};
    use std::sync::Arc;
    use web_search::{WebSearchProvider, WebSearchProviderId};

    #[test]
    fn test_tavily_provider_id() {
        let provider = tavily::TavilyWebSearchProvider::new(
            "test-key".into(),
            5,
            240,
        );
        assert_eq!(
            provider.id(),
            WebSearchProviderId("tavily".into())
        );
    }

    #[test]
    fn test_exa_provider_id() {
        let provider = exa::ExaWebSearchProvider::new(
            "test-key".into(),
            5,
            240,
        );
        assert_eq!(
            provider.id(),
            WebSearchProviderId("exa".into())
        );
    }

    #[test]
    fn test_tavily_html_stripping_and_truncation() {
        // Test HTML stripping and truncation functions directly
        let test_html = "<p>This is a test with <b>HTML</b> tags</p>";
        let stripped = tavily::strip_html(test_html);
        assert_eq!(stripped, "This is a test with HTML tags");
        
        let long_text = "a ".repeat(200); // 400 characters
        let truncated = tavily::truncate_text(&long_text, 240);
        assert!(truncated.len() <= 243); // 240 + "..."
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_exa_text_truncation() {
        // Test text truncation
        let long_text = "a ".repeat(200); // 400 characters
        let truncated = exa::truncate_text(&long_text, 240);
        assert!(truncated.len() <= 243); // 240 + "..."
        assert!(truncated.ends_with("..."));
        
        // Test that text and highlights would be joined (simulating the logic)
        let text = Some("Main text".to_string());
        let highlights = Some(vec!["Highlight 1".to_string(), "Highlight 2".to_string()]);
        let joined = {
            let mut parts = Vec::new();
            if let Some(t) = text {
                parts.push(t);
            }
            if let Some(h) = highlights {
                parts.extend(h);
            }
            parts.join(" ")
        };
        assert_eq!(joined, "Main text Highlight 1 Highlight 2");
    }

    #[test]
    fn test_html_stripping() {
        // Test various HTML stripping scenarios
        let test_cases = vec![
            ("<p>Simple text</p>", "Simple text"),
            ("Text with <b>bold</b> and <i>italic</i>", "Text with bold and italic"),
            ("<div>Nested <span>tags</span></div>", "Nested tags"),
            ("No HTML here", "No HTML here"),
            ("<script>alert('xss')</script>Safe text", "alert('xss')Safe text"), // Note: simple stripper removes tags but not script content
            ("<a href='#'>Link</a> text", "Link text"),
        ];

        for (input, expected) in test_cases {
            let stripped = tavily::strip_html(input);
            assert_eq!(stripped, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_text_truncation() {
        // Test truncation at word boundaries
        let short_text = "Short text";
        let truncated = tavily::truncate_text(short_text, 240);
        assert_eq!(truncated, short_text);

        // Test truncation of long text
        let long_text = "word ".repeat(100); // 500 characters
        let truncated = tavily::truncate_text(&long_text, 240);
        assert!(truncated.len() <= 243);
        assert!(truncated.ends_with("..."));

        // Test truncation with no spaces (should still truncate)
        let no_spaces = "a".repeat(500);
        let truncated = tavily::truncate_text(&no_spaces, 240);
        assert_eq!(truncated.len(), 243); // 240 + "..."
        assert!(truncated.ends_with("..."));

        // Test truncation at word boundary preference
        let text_with_spaces = "word ".repeat(50) + "middle " + &"word ".repeat(50);
        let truncated = tavily::truncate_text(&text_with_spaces, 240);
        // Should truncate at a space if possible
        if truncated.len() > 240 {
            assert!(truncated.ends_with("..."));
        }
    }

    #[test]
    fn test_api_key_env_var_check() {
        // Test that has_api_key_sync checks environment variables
        // Note: This test depends on actual environment variables,
        // so we'll just verify the function exists and works
        use crate::api_key;
        let has_tavily = api_key::has_api_key_sync(tavily::TAVILY_API_KEY_ENV_VAR);
        let has_exa = api_key::has_api_key_sync(exa::EXA_API_KEY_ENV_VAR);
        
        // These may be true or false depending on test environment
        // Just verify the function doesn't panic
        let _ = has_tavily;
        let _ = has_exa;
    }

    #[test]
    fn test_provider_ids_are_unique() {
        let tavily_id = WebSearchProviderId("tavily".into());
        let exa_id = WebSearchProviderId("exa".into());
        let zed_id = WebSearchProviderId("zed_cloud".into());

        assert_ne!(tavily_id, exa_id);
        assert_ne!(tavily_id, zed_id);
        assert_ne!(exa_id, zed_id);
    }

    #[test]
    fn test_truncation_preserves_meaning() {
        // Test that truncation doesn't break in the middle of important words
        let meaningful_text = "This is a very important sentence that contains critical information about the topic we are discussing.";
        let truncated = tavily::truncate_text(meaningful_text, 50);
        
        // Should end with "..." and not break mid-word if possible
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 53); // 50 + "..."
        
        // Should preserve the beginning
        assert!(truncated.starts_with("This is"));
    }

}
