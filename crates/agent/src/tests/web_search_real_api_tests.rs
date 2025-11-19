// Integration tests that use real Tavily and Exa APIs
// These tests require valid API keys and make real HTTP requests
// Run with: cargo test --package agent --lib -- --ignored

use super::*;
use reqwest_client::ReqwestClient;
use std::sync::Arc;
use web_search::WebSearchProvider;
use web_search_providers::{exa, tavily};

// Real API keys for testing
const TAVILY_API_KEY: &str = "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";
const EXA_API_KEY: &str = "92083e68-6aa1-4eaa-b8fe-b62855000a2c";

#[gpui::test]
#[ignore] // Ignore by default - requires API keys and makes real HTTP calls
async fn test_tavily_real_api_search(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    
    // Set up real HTTP client
    cx.update(|cx| {
        let http_client = ReqwestClient::user_agent("web_search_providers_test").unwrap();
        cx.set_http_client(Arc::new(http_client));
    });
    
    // Create Tavily provider with real API key
    let provider = Arc::new(tavily::TavilyWebSearchProvider::new(
        TAVILY_API_KEY.into(),
        5,
        300, // Use 300 chars for snippet length
    ));
    
    // Perform real search
    let task = cx.update(|cx| {
        provider.search("Rust programming language".to_string(), cx)
    });
    
    let search_response = task.await.expect("Search should succeed");
    
    // Verify we got real results
    assert!(!search_response.results.is_empty(), "Should return at least one result");
    
    // Verify results have real URLs (not example.com)
    for result in &search_response.results {
        assert!(!result.url.is_empty(), "Result should have a URL");
        assert!(
            !result.url.contains("example.com"),
            "Should have real URLs, not example.com. Got: {}",
            result.url
        );
        assert!(
            result.url.starts_with("http://") || result.url.starts_with("https://"),
            "URL should be valid. Got: {}",
            result.url
        );
        
        // Verify title and text are present
        assert!(!result.title.is_empty(), "Result should have a title");
        assert!(!result.text.is_empty(), "Result should have text content");
        
        println!("✅ Real Tavily Result:");
        println!("   Title: {}", result.title);
        println!("   URL: {}", result.url);
        println!("   Text (first 200 chars): {}", &result.text[..result.text.len().min(200)]);
    }
}

#[gpui::test]
#[ignore] // Ignore by default - requires API keys and makes real HTTP calls
async fn test_exa_real_api_search(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    
    // Set up real HTTP client
    cx.update(|cx| {
        let http_client = ReqwestClient::user_agent("web_search_providers_test").unwrap();
        cx.set_http_client(Arc::new(http_client));
    });
    
    // Create Exa provider with real API key
    let provider = Arc::new(exa::ExaWebSearchProvider::new(
        EXA_API_KEY.into(),
        5,
        300, // Use 300 chars for snippet length
    ));
    
    // Perform real search
    let result = cx.update(|cx| {
        provider.search("Python programming language".to_string(), cx)
    });
    
    let search_response = result.await.expect("Search should succeed");
    
    // Verify we got real results
    assert!(!search_response.results.is_empty(), "Should return at least one result");
    
    // Verify results have real URLs (not example.com)
    for result in &search_response.results {
        assert!(!result.url.is_empty(), "Result should have a URL");
        assert!(
            !result.url.contains("example.com"),
            "Should have real URLs, not example.com. Got: {}",
            result.url
        );
        assert!(
            result.url.starts_with("http://") || result.url.starts_with("https://"),
            "URL should be valid. Got: {}",
            result.url
        );
        
            // Verify title is present (text may be empty for some Exa results)
            assert!(!result.title.is_empty(), "Result should have a title");
            
            println!("✅ Real Exa Result:");
            println!("   Title: {}", result.title);
            println!("   URL: {}", result.url);
            if !result.text.is_empty() {
                println!("   Text (first 200 chars): {}", &result.text[..result.text.len().min(200)]);
            } else {
                println!("   Text: (empty - Exa may return results without text content)");
            }
    }
}

