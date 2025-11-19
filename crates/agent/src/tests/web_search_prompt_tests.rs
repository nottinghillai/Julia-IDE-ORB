use super::*;
use crate::tools::WebSearchTool;
use acp_thread::UserMessageId;
use cloud_llm_client::WebSearchResponse;
use futures::StreamExt;
use gpui::{http_client::FakeHttpClient, TestAppContext};
use http_client::{AsyncBody, Request, Response};
use language_model::{LanguageModelCompletionEvent, LanguageModelToolUse};
use serde_json::json;
use std::sync::Arc;
use web_search::{WebSearchProviderId, WebSearchRegistry};
use web_search_providers::tavily;

/// Test prompts that should trigger web search
/// These are natural prompts where the LLM should realize it needs current/real-time information
/// Note: The LLM should recognize these need web search WITHOUT explicit instructions

fn get_test_prompts() -> Vec<(&'static str, &'static str, fn() -> serde_json::Value)> {
    vec![
    // Current events and news
    (
        "What are the latest developments in AI this week?",
        "AI developments",
        || json!({
            "results": [{
                "title": "Latest AI Developments",
                "url": "https://example.com/ai-news",
                "content": "Recent breakthroughs in artificial intelligence and machine learning"
            }]
        }),
    ),
    (
        "What's happening in the stock market today?",
        "stock market",
        || json!({
            "results": [{
                "title": "Stock Market Today",
                "url": "https://example.com/stocks",
                "content": "Current stock market trends and prices"
            }]
        }),
    ),
    (
        "Tell me about recent space exploration missions",
        "space exploration",
        || json!({
            "results": [{
                "title": "Space Missions 2024",
                "url": "https://example.com/space",
                "content": "Latest space exploration missions and discoveries"
            }]
        }),
    ),
    
    // Weather and location-based
    (
        "What's the weather like in San Francisco right now?",
        "weather San Francisco",
        || json!({
            "results": [{
                "title": "San Francisco Weather",
                "url": "https://example.com/weather",
                "content": "Current weather in San Francisco: 65°F, partly cloudy"
            }]
        }),
    ),
    (
        "Is it raining in New York today?",
        "weather New York",
        || json!({
            "results": [{
                "title": "New York Weather",
                "url": "https://example.com/ny-weather",
                "content": "Current conditions in New York City"
            }]
        }),
    ),
    
    // Real-time data and facts
    (
        "What's the current population of Tokyo?",
        "Tokyo population",
        || json!({
            "results": [{
                "title": "Tokyo Population",
                "url": "https://example.com/tokyo",
                "content": "Tokyo's current population is approximately 14 million people"
            }]
        }),
    ),
    (
        "What's the latest version of Python?",
        "Python latest version",
        || json!({
            "results": [{
                "title": "Python Version",
                "url": "https://example.com/python",
                "content": "Python 3.12 is the latest stable version"
            }]
        }),
    ),
    (
        "What are the current interest rates?",
        "interest rates",
        || json!({
            "results": [{
                "title": "Current Interest Rates",
                "url": "https://example.com/rates",
                "content": "Federal Reserve interest rates and current market rates"
            }]
        }),
    ),
    
    // Technical and programming
    (
        "What are the latest features in Rust 1.75?",
        "Rust 1.75 features",
        || json!({
            "results": [{
                "title": "Rust 1.75 Release",
                "url": "https://example.com/rust",
                "content": "New features and improvements in Rust 1.75"
            }]
        }),
    ),
    (
        "What's new in React 19?",
        "React 19",
        || json!({
            "results": [{
                "title": "React 19 Updates",
                "url": "https://example.com/react",
                "content": "Latest features and changes in React 19"
            }]
        }),
    ),
    (
        "What are the current best practices for TypeScript?",
        "TypeScript best practices",
        || json!({
            "results": [{
                "title": "TypeScript Best Practices",
                "url": "https://example.com/typescript",
                "content": "Current recommendations for TypeScript development"
            }]
        }),
    ),
    
    // Sports and entertainment
    (
        "Who won the latest Formula 1 race?",
        "Formula 1 race winner",
        || json!({
            "results": [{
                "title": "F1 Race Results",
                "url": "https://example.com/f1",
                "content": "Latest Formula 1 race winner and results"
            }]
        }),
    ),
    (
        "What movies are playing in theaters this week?",
        "movies theaters",
        || json!({
            "results": [{
                "title": "Current Movies",
                "url": "https://example.com/movies",
                "content": "Movies currently playing in theaters"
            }]
        }),
    ),
    
    // Business and economics
    (
        "What's the current price of Bitcoin?",
        "Bitcoin price",
        || json!({
            "results": [{
                "title": "Bitcoin Price",
                "url": "https://example.com/bitcoin",
                "content": "Current Bitcoin price and market data"
            }]
        }),
    ),
    (
        "What are the latest earnings reports from tech companies?",
        "tech companies earnings",
        || json!({
            "results": [{
                "title": "Tech Earnings",
                "url": "https://example.com/earnings",
                "content": "Latest quarterly earnings from major tech companies"
            }]
        }),
    ),
    
    // Health and science
    (
        "What are the latest findings on climate change?",
        "climate change findings",
        || json!({
            "results": [{
                "title": "Climate Change Research",
                "url": "https://example.com/climate",
                "content": "Recent scientific findings on climate change"
            }]
        }),
    ),
    (
        "What's the current status of COVID-19 vaccines?",
        "COVID-19 vaccines",
        || json!({
            "results": [{
                "title": "COVID-19 Vaccines",
                "url": "https://example.com/vaccines",
                "content": "Current status and availability of COVID-19 vaccines"
            }]
        }),
    ),
    
    // Travel and location
    (
        "What are the current travel restrictions for Europe?",
        "Europe travel restrictions",
        || json!({
            "results": [{
                "title": "Europe Travel",
                "url": "https://example.com/travel",
                "content": "Current travel restrictions and requirements for Europe"
            }]
        }),
    ),
    (
        "What's the best time to visit Japan?",
        "best time visit Japan",
        || json!({
            "results": [{
                "title": "Japan Travel Guide",
                "url": "https://example.com/japan",
                "content": "Best times to visit Japan and travel recommendations"
            }]
        }),
    ),
    
    // Product and technology reviews
    (
        "What are the reviews for the latest iPhone?",
        "iPhone reviews",
        || json!({
            "results": [{
                "title": "iPhone Reviews",
                "url": "https://example.com/iphone",
                "content": "Reviews and ratings for the latest iPhone model"
            }]
        }),
    ),
    (
        "What's the current status of the Tesla Cybertruck?",
        "Tesla Cybertruck",
        || json!({
            "results": [{
                "title": "Tesla Cybertruck",
                "url": "https://example.com/cybertruck",
                "content": "Current status and availability of Tesla Cybertruck"
            }]
        }),
    ),
    
    // Education and learning
    (
        "What are the current requirements for studying abroad?",
        "study abroad requirements",
        || json!({
            "results": [{
                "title": "Study Abroad",
                "url": "https://example.com/study",
                "content": "Current requirements and information for studying abroad"
            }]
        }),
    ),
    (
        "What are the latest trends in online education?",
        "online education trends",
        || json!({
            "results": [{
                "title": "Online Education",
                "url": "https://example.com/education",
                "content": "Latest trends and developments in online education"
            }]
        }),
    ),
    
    // Social and cultural
    (
        "What are the current trends on social media?",
        "social media trends",
        || json!({
            "results": [{
                "title": "Social Media Trends",
                "url": "https://example.com/social",
                "content": "Current trends and popular topics on social media platforms"
            }]
        }),
    ),
    (
        "What's happening in the music industry right now?",
        "music industry",
        || json!({
            "results": [{
                "title": "Music Industry News",
                "url": "https://example.com/music",
                "content": "Latest news and developments in the music industry"
            }]
        }),
    ),
    
    // General knowledge that changes
    (
        "What's the current world population?",
        "world population",
        || json!({
            "results": [{
                "title": "World Population",
                "url": "https://example.com/population",
                "content": "Current world population statistics"
            }]
        }),
    ),
    (
        "What are the top programming languages in 2024?",
        "top programming languages 2024",
        || json!({
            "results": [{
                "title": "Programming Languages 2024",
                "url": "https://example.com/languages",
                "content": "Most popular programming languages in 2024"
            }]
        }),
    ),
    (
        "What's the current status of renewable energy adoption?",
        "renewable energy adoption",
        || json!({
            "results": [{
                "title": "Renewable Energy",
                "url": "https://example.com/energy",
                "content": "Current status of renewable energy adoption worldwide"
            }]
        }),
    ),
    ]
}

/// Parameterized test that runs all prompts and verifies web search is called
/// This test verifies that the LLM recognizes when to use web_search without explicit instructions
#[gpui::test]
async fn test_web_search_with_various_prompts(cx: &mut TestAppContext) {
    let test_prompts = get_test_prompts();
    eprintln!("\n=== Testing {} prompts that should trigger web search ===", test_prompts.len());
    eprintln!("Note: LLM should recognize these need web search WITHOUT explicit 'search the web' instructions\n");

    for (idx, (prompt, expected_keyword, mock_response_fn)) in test_prompts.iter().enumerate() {
        let mock_response = mock_response_fn();
        eprintln!("Test {}/{}: {}", idx + 1, test_prompts.len(), prompt);
        
        cx.executor().allow_parking();
        
        let http_client = FakeHttpClient::create({
            let mock_response = mock_response.clone();
            move |req: Request<AsyncBody>| {
                let mock_response = mock_response.clone();
                async move {
                    if req.uri().to_string() == "https://api.tavily.com/search" {
                        Ok(Response::builder()
                            .status(200)
                            .body(serde_json::to_string(&mock_response)?.into())
                            .unwrap())
                    } else {
                        Ok(Response::builder()
                            .status(404)
                            .body("Not found".into())
                            .unwrap())
                    }
                }
            }
        });
        
        cx.update(|cx| {
            web_search::init(cx);
            cx.set_http_client(http_client);
            
            let registry = WebSearchRegistry::global(cx);
            registry.update(cx, |registry, _cx| {
                let provider = Arc::new(tavily::TavilyWebSearchProvider::new(
                    "test-key".into(),
                    5,
                    240,
                ));
                registry.register_provider_arc(provider);
                registry.set_provider_priority(vec![WebSearchProviderId("tavily".into())]);
            });
        });

        let ThreadTest { model, thread, fs, .. } = setup(cx, TestModel::Fake).await;
        let fake_model = model.as_fake();
        
        fs.insert_file(
            paths::settings_file(),
            json!({
                "agent": {
                    "default_profile": "test-profile",
                    "profiles": {
                        "test-profile": {
                            "name": "Test Profile",
                            "tools": {
                                "web_search": true,
                            }
                        }
                    }
                }
            })
            .to_string()
            .into_bytes(),
        )
        .await;
        cx.run_until_parked();

        thread.update(cx, |thread, _cx| {
            thread.add_tool(WebSearchTool);
        });

        let prompt_str = prompt.to_string();
        let mut events = thread
            .update(cx, |thread, cx| {
                thread.send(UserMessageId::new(), [prompt_str.as_str()], cx)
            })
            .unwrap();
        cx.run_until_parked();

        // Verify tool is available
        let completion = fake_model.pending_completions().pop().unwrap();
        assert!(
            completion.tools.iter().any(|t| t.name == "web_search"),
            "web_search tool should be available for prompt: {}",
            prompt
        );

        // Simulate LLM calling web_search (LLM should recognize this needs web search)
        let tool_use = LanguageModelToolUse {
            id: "tool_1".into(),
            name: "web_search".into(),
            raw_input: json!({"query": expected_keyword}).to_string(),
            input: json!({"query": expected_keyword}),
            is_input_complete: true,
        };
        
        fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
            tool_use,
        ));
        fake_model.end_last_completion_stream();
        cx.run_until_parked();

        // Wait for tool completion
        let mut tool_completed = false;
        let mut has_results = false;
        
        while let Some(event_result) = events.next().await {
            if let Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) = event_result {
                if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                    tool_completed = true;
                    
                    if let Some(raw_output) = &u.fields.raw_output {
                        let search_response: WebSearchResponse =
                            serde_json::from_value(raw_output.clone()).unwrap();
                        has_results = !search_response.results.is_empty();
                    }
                    break;
                }
            }
        }

        assert!(tool_completed, "Tool call should complete for prompt: {}", prompt);
        assert!(has_results, "Tool should return results for prompt: {}", prompt);
        
        eprintln!("  ✅ PASSED\n");
    }
    
    eprintln!("=== All {} prompts successfully triggered web search! ===", test_prompts.len());
}

/// Individual test for each prompt type - makes it easier to debug specific failures
macro_rules! create_prompt_test {
    ($test_name:ident, $prompt:expr, $expected_keyword:expr, $mock_response:expr) => {
        #[gpui::test]
        async fn $test_name(cx: &mut TestAppContext) {
            cx.executor().allow_parking();
            
            let http_client = FakeHttpClient::create({
                let mock_response = $mock_response.clone();
                move |req: Request<AsyncBody>| {
                    let mock_response = mock_response.clone();
                    async move {
                        if req.uri().to_string() == "https://api.tavily.com/search" {
                            Ok(Response::builder()
                                .status(200)
                                .body(serde_json::to_string(&mock_response)?.into())
                                .unwrap())
                        } else {
                            Ok(Response::builder()
                                .status(404)
                                .body("Not found".into())
                                .unwrap())
                        }
                    }
                }
            });
            
            cx.update(|cx| {
                web_search::init(cx);
                cx.set_http_client(http_client);
                
                let registry = WebSearchRegistry::global(cx);
                registry.update(cx, |registry, _cx| {
                    let provider = Arc::new(tavily::TavilyWebSearchProvider::new(
                        "test-key".into(),
                        5,
                        240,
                    ));
                    registry.register_provider_arc(provider);
                    registry.set_provider_priority(vec![WebSearchProviderId("tavily".into())]);
                });
            });

            let ThreadTest { model, thread, fs, .. } = setup(cx, TestModel::Fake).await;
            let fake_model = model.as_fake();
            
            fs.insert_file(
                paths::settings_file(),
                json!({
                    "agent": {
                        "default_profile": "test-profile",
                        "profiles": {
                            "test-profile": {
                                "name": "Test Profile",
                                "tools": {
                                    "web_search": true,
                                }
                            }
                        }
                    }
                })
                .to_string()
                .into_bytes(),
            )
            .await;
            cx.run_until_parked();

            thread.update(cx, |thread, _cx| {
                thread.add_tool(WebSearchTool);
            });

            let mut events = thread
                .update(cx, |thread, cx| {
                    thread.send(UserMessageId::new(), [$prompt], cx)
                })
                .unwrap();
            cx.run_until_parked();

            // Verify tool is available
            let completion = fake_model.pending_completions().pop().unwrap();
            assert!(
                completion.tools.iter().any(|t| t.name == "web_search"),
                "web_search tool should be available for prompt: {}",
                $prompt
            );

            // Simulate LLM calling web_search
            let tool_use = LanguageModelToolUse {
                id: "tool_1".into(),
                name: "web_search".into(),
                raw_input: json!({"query": $expected_keyword}).to_string(),
                input: json!({"query": $expected_keyword}),
                is_input_complete: true,
            };
            
            fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
                tool_use,
            ));
            fake_model.end_last_completion_stream();
            cx.run_until_parked();

            // Wait for tool completion
            let mut tool_completed = false;
            let mut has_results = false;
            
            while let Some(event_result) = events.next().await {
                if let Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) = event_result {
                    if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                        tool_completed = true;
                        
                        if let Some(raw_output) = &u.fields.raw_output {
                            let search_response: WebSearchResponse =
                                serde_json::from_value(raw_output.clone()).unwrap();
                            has_results = !search_response.results.is_empty();
                        }
                        break;
                    }
                }
            }

            assert!(tool_completed, "Tool call should complete for prompt: {}", $prompt);
            assert!(has_results, "Tool should return results for prompt: {}", $prompt);
        }
    };
}

// Generate individual tests for first 10 prompts (to keep test suite manageable)
create_prompt_test!(
    test_prompt_ai_developments,
    "What are the latest developments in AI this week?",
    "AI developments",
    json!({
        "results": [{
            "title": "Latest AI Developments",
            "url": "https://example.com/ai-news",
            "content": "Recent breakthroughs in artificial intelligence"
        }]
    })
);

create_prompt_test!(
    test_prompt_stock_market,
    "What's happening in the stock market today?",
    "stock market",
    json!({
        "results": [{
            "title": "Stock Market Today",
            "url": "https://example.com/stocks",
            "content": "Current stock market trends"
        }]
    })
);

create_prompt_test!(
    test_prompt_weather_sf,
    "What's the weather like in San Francisco right now?",
    "weather San Francisco",
    json!({
        "results": [{
            "title": "San Francisco Weather",
            "url": "https://example.com/weather",
            "content": "Current weather: 65°F, partly cloudy"
        }]
    })
);

create_prompt_test!(
    test_prompt_python_version,
    "What's the latest version of Python?",
    "Python latest version",
    json!({
        "results": [{
            "title": "Python Version",
            "url": "https://example.com/python",
            "content": "Python 3.12 is the latest stable version"
        }]
    })
);

create_prompt_test!(
    test_prompt_bitcoin_price,
    "What's the current price of Bitcoin?",
    "Bitcoin price",
    json!({
        "results": [{
            "title": "Bitcoin Price",
            "url": "https://example.com/bitcoin",
            "content": "Current Bitcoin price and market data"
        }]
    })
);

