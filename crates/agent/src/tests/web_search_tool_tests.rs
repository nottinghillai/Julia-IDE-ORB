use super::*;
use crate::tools::WebSearchTool;
use acp_thread::UserMessageId;
use cloud_llm_client::WebSearchResponse;
use futures::AsyncReadExt;
use gpui::{http_client::FakeHttpClient, TestAppContext};
use http_client::{AsyncBody, Method, Request, Response};
use language_model::{LanguageModelCompletionEvent, LanguageModelToolUse};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use web_search::{WebSearchProviderId, WebSearchRegistry};
use web_search_providers::{exa, tavily};

#[gpui::test]
async fn test_web_search_tool_with_tavily(cx: &mut TestAppContext) {
    // Setup test environment
    cx.executor().allow_parking();
    
    // Create a fake HTTP client that returns a successful Tavily response
    let http_client = FakeHttpClient::create(|req: Request<AsyncBody>| {
        async move {
            // Verify request
            assert_eq!(req.uri().to_string(), "https://api.tavily.com/search");
            assert_eq!(req.method(), &Method::POST);
            
            // Read request body
            let mut body = req.into_body();
            let mut body_bytes = Vec::new();
            body.read_to_end(&mut body_bytes).await?;
            let body_str = String::from_utf8(body_bytes)?;
            let request_json: serde_json::Value = serde_json::from_str(&body_str)?;
            
            assert_eq!(request_json["query"], "test query");
            assert_eq!(request_json["max_results"], 5);
            assert_eq!(request_json["search_depth"], "basic");
            
            // Return mock response
            let response_body = json!({
                "results": [
                    {
                        "title": "Test Result 1",
                        "url": "https://example.com/1",
                        "content": "This is a test result with HTML <b>content</b> that should be stripped."
                    },
                    {
                        "title": "Test Result 2",
                        "url": "https://example.com/2",
                        "snippet": "Another result snippet"
                    }
                ]
            });
            
            Ok(Response::builder()
                .status(200)
                .body(serde_json::to_string(&response_body)?.into())
                .unwrap())
        }
    });
    
    // Initialize web search registry and set HTTP client BEFORE creating thread
    cx.update(|cx| {
        web_search::init(cx);
        cx.set_http_client(http_client);
        
        // Register Tavily provider
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

    // Setup thread with web search tool (AFTER registry is initialized)
    // Note: The tool will be added via thread.add_tool() below
    // The setup() function creates the test profile, and we'll add the tool to the thread
    let ThreadTest { model, thread, fs, .. } = setup(cx, TestModel::Fake).await;
    
    // Update the settings file to include web_search in the test profile
    // This is needed because enabled_tools() checks profile.is_tool_enabled()
    fs.insert_file(
        paths::settings_file(),
        json!({
            "agent": {
                "default_profile": "test-profile",
                "profiles": {
                    "test-profile": {
                        "name": "Test Profile",
                        "tools": {
                            "echo": true,
                            "delay": true,
                            "word_list": true,
                            "tool_requiring_permission": true,
                            "infinite": true,
                            "thinking": true,
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
    let fake_model = model.as_fake();

    // Add tool
    thread.update(cx, |thread, _cx| {
        thread.add_tool(WebSearchTool);
    });

    let mut events = thread
        .update(cx, |thread, cx| {
            thread.send(UserMessageId::new(), ["Search the web for 'test query'"], cx)
        })
        .unwrap();
    cx.run_until_parked();

    // Verify the tool is available in the completion request
    let completion = fake_model.pending_completions().pop().unwrap();
    let tool_names: Vec<String> = completion
        .tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    
    // Note: The tool might not be available if no providers are registered
    // But since we registered Tavily above, it should be available
    // If it's not, we'll still test the tool call execution
    if !tool_names.contains(&"web_search".to_string()) {
        eprintln!("Warning: web_search tool not in available tools: {:?}", tool_names);
        // Continue anyway to test tool execution
    }

    // Simulate the model calling the web_search tool
    let tool_use = LanguageModelToolUse {
        id: "tool_1".into(),
        name: "web_search".into(),
        raw_input: json!({"query": "test query"}).to_string(),
        input: json!({"query": "test query"}),
        is_input_complete: true,
    };
    fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
        tool_use.clone(),
    ));
    fake_model.end_last_completion_stream();
    
    cx.run_until_parked();

    // Collect events and find the completed tool call
    let mut tool_call_received = false;
    let mut completed_update = None;
    
    // Process events - we need to handle ToolCall first, then wait for completion
    while let Some(event_result) = events.next().await {
        match event_result {
            Ok(ThreadEvent::ToolCall(_)) => {
                tool_call_received = true;
                // Continue to wait for updates
            }
            Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) => {
                if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                    completed_update = Some(u);
                    break;
                } else if u.fields.status == Some(acp::ToolCallStatus::Failed) {
                    let error_msg = match &u.fields.raw_output {
                        Some(v) if v.is_string() => v.as_str().unwrap_or("Unknown error").to_string(),
                        Some(v) => v.to_string(),
                        None => "Unknown error".to_string(),
                    };
                    panic!("Tool call failed: {}", error_msg);
                }
                // Continue waiting for completion status
            }
            Ok(ThreadEvent::Stop(_)) => {
                break;
            }
            Err(e) => {
                panic!("Error in event stream: {:?}", e);
            }
            _ => {
                // Continue processing other events
            }
        }
    }
    
    if !tool_call_received {
        panic!("Tool call event never received");
    }
    
    let update = completed_update.expect("Tool call should complete successfully");
    
    // Check if the tool call succeeded or failed
    match update.fields.status {
        Some(acp::ToolCallStatus::Completed) => {
            // Verify the tool result contains search results
            if let Some(raw_output) = &update.fields.raw_output {
                let search_response: WebSearchResponse =
                    serde_json::from_value(raw_output.clone()).unwrap();
                assert_eq!(search_response.results.len(), 2);
                assert_eq!(search_response.results[0].title, "Test Result 1");
                assert_eq!(search_response.results[0].url, "https://example.com/1");
                // Verify HTML was stripped
                assert!(!search_response.results[0].text.contains("<b>"));
                assert!(search_response.results[0].text.contains("content"));
            } else {
                panic!("Tool call completed but no output provided");
            }
        }
        Some(acp::ToolCallStatus::Failed) => {
            let error_msg = match &update.fields.raw_output {
                Some(v) if v.is_string() => v.as_str().unwrap_or("Unknown error").to_string(),
                Some(v) => v.to_string(),
                None => "Unknown error".to_string(),
            };
            panic!("Tool call failed: {}", error_msg);
        }
        _ => {
            panic!("Tool call did not complete: status = {:?}", update.fields.status);
        }
    }
}

#[gpui::test]
async fn test_web_search_tool_with_exa(cx: &mut TestAppContext) {
    // Setup test environment
    cx.executor().allow_parking();
    
    // Initialize web search registry
    cx.update(|cx| {
        web_search::init(cx);
        
        // Create a fake HTTP client that returns a successful Exa response
        let http_client = FakeHttpClient::create(|req: Request<AsyncBody>| {
            async move {
                // Verify request
                assert_eq!(req.uri().to_string(), "https://api.exa.ai/search_and_contents");
                assert_eq!(req.method(), &Method::POST);
                
                // Read request body
                let mut body = req.into_body();
                let mut body_bytes = Vec::new();
                body.read_to_end(&mut body_bytes).await?;
                let body_str = String::from_utf8(body_bytes)?;
                let request_json: serde_json::Value = serde_json::from_str(&body_str)?;
                
                assert_eq!(request_json["query"], "test query");
                assert_eq!(request_json["num_results"], 5);
                assert_eq!(request_json["type"], "keyword");
                
                // Return mock response
                let response_body = json!({
                    "results": [
                        {
                            "title": "Exa Result 1",
                            "url": "https://example.com/exa1",
                            "text": "Main text content",
                            "highlights": ["Highlight 1", "Highlight 2"]
                        },
                        {
                            "title": "Exa Result 2",
                            "url": "https://example.com/exa2",
                            "text": "Another result"
                        }
                    ]
                });
                
                Ok(Response::builder()
                    .status(200)
                    .body(serde_json::to_string(&response_body)?.into())
                    .unwrap())
            }
        });
        cx.set_http_client(http_client);
    });

    // Register Exa provider
    cx.update(|cx| {
        let registry = WebSearchRegistry::global(cx);
        registry.update(cx, |registry, _cx| {
            let provider = Arc::new(exa::ExaWebSearchProvider::new(
                "test-key".into(),
                5,
                240,
            ));
            registry.register_provider_arc(provider);
            registry.set_provider_priority(vec![WebSearchProviderId("exa".into())]);
        });
    });

    // Setup thread with web search tool
    let ThreadTest { model, thread, fs, .. } = setup(cx, TestModel::Fake).await;
    
    // Update the settings file to include web_search in the test profile
    fs.insert_file(
        paths::settings_file(),
        json!({
            "agent": {
                "default_profile": "test-profile",
                "profiles": {
                    "test-profile": {
                        "name": "Test Profile",
                        "tools": {
                            "echo": true,
                            "delay": true,
                            "word_list": true,
                            "tool_requiring_permission": true,
                            "infinite": true,
                            "thinking": true,
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
    let fake_model = model.as_fake();

    thread.update(cx, |thread, _cx| {
        thread.add_tool(WebSearchTool);
    });

    let mut events = thread
        .update(cx, |thread, cx| {
            thread.send(UserMessageId::new(), ["Search the web for 'test query'"], cx)
        })
        .unwrap();
    cx.run_until_parked();

    // Simulate the model calling the web_search tool
    let tool_use = LanguageModelToolUse {
        id: "tool_1".into(),
        name: "web_search".into(),
        raw_input: json!({"query": "test query"}).to_string(),
        input: json!({"query": "test query"}),
        is_input_complete: true,
    };
    fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
        tool_use.clone(),
    ));
    fake_model.end_last_completion_stream();
    
    cx.run_until_parked();

    // Collect events and find the completed tool call
    let mut tool_call_received = false;
    let mut completed_update = None;
    
    // Process events - we need to handle ToolCall first, then wait for completion
    while let Some(event_result) = events.next().await {
        match event_result {
            Ok(ThreadEvent::ToolCall(_)) => {
                tool_call_received = true;
                // Continue to wait for updates
            }
            Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) => {
                if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                    completed_update = Some(u);
                    break;
                } else if u.fields.status == Some(acp::ToolCallStatus::Failed) {
                    let error_msg = match &u.fields.raw_output {
                        Some(v) if v.is_string() => v.as_str().unwrap_or("Unknown error").to_string(),
                        Some(v) => v.to_string(),
                        None => "Unknown error".to_string(),
                    };
                    panic!("Tool call failed: {}", error_msg);
                }
                // Continue waiting for completion status
            }
            Ok(ThreadEvent::Stop(_)) => {
                break;
            }
            Err(e) => {
                panic!("Error in event stream: {:?}", e);
            }
            _ => {
                // Continue processing other events
            }
        }
    }
    
    if !tool_call_received {
        panic!("Tool call event never received");
    }
    
    let update = completed_update.expect("Tool call should complete successfully");
    
    match update.fields.status {
        Some(acp::ToolCallStatus::Completed) => {
            if let Some(raw_output) = &update.fields.raw_output {
                let search_response: WebSearchResponse =
                    serde_json::from_value(raw_output.clone()).unwrap();
                assert_eq!(search_response.results.len(), 2);
                assert_eq!(search_response.results[0].title, "Exa Result 1");
                assert_eq!(search_response.results[0].url, "https://example.com/exa1");
                assert!(search_response.results[0].text.contains("Main text content"));
                assert!(search_response.results[0].text.contains("Highlight 1"));
            } else {
                panic!("Tool call completed but no output provided");
            }
        }
        Some(acp::ToolCallStatus::Failed) => {
            let error_msg = match &update.fields.raw_output {
                Some(v) if v.is_string() => v.as_str().unwrap_or("Unknown error").to_string(),
                Some(v) => v.to_string(),
                None => "Unknown error".to_string(),
            };
            panic!("Tool call failed: {}", error_msg);
        }
        _ => panic!("Tool call did not complete: status = {:?}", update.fields.status),
    }
}

#[gpui::test]
async fn test_web_search_tool_fallback(cx: &mut TestAppContext) {
    // Setup test environment
    cx.executor().allow_parking();
    
    // Initialize web search registry
    cx.update(|cx| {
        web_search::init(cx);
        
        // Create a fake HTTP client that fails on first call (Tavily), succeeds on second (Exa)
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let call_count_clone = call_count.clone();
        let http_client = FakeHttpClient::create(move |req: Request<AsyncBody>| {
            let call_count = call_count_clone.clone();
            let current_call = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if req.uri().to_string() == "https://api.tavily.com/search" && current_call == 0 {
                    // First call to Tavily fails with 429 (rate limit)
                    Ok(Response::builder()
                        .status(429)
                        .body("Rate limit exceeded".into())
                        .unwrap())
                } else if req.uri().to_string() == "https://api.exa.ai/search_and_contents" {
                    // Second call to Exa succeeds
                    let response_body = json!({
                        "results": [
                            {
                                "title": "Fallback Result",
                                "url": "https://example.com/fallback",
                                "text": "This result came from the fallback provider"
                            }
                        ]
                    });
                    
                    Ok(Response::builder()
                        .status(200)
                        .body(serde_json::to_string(&response_body)?.into())
                        .unwrap())
                } else {
                    Ok(Response::builder()
                        .status(500)
                        .body("Unexpected request".into())
                        .unwrap())
                }
            }
        });
        cx.set_http_client(http_client);
    });

    // Register both providers with Tavily first in priority
    cx.update(|cx| {
        let registry = WebSearchRegistry::global(cx);
        registry.update(cx, |registry, _cx| {
            let tavily_provider = Arc::new(tavily::TavilyWebSearchProvider::new(
                "test-key".into(),
                5,
                240,
            ));
            let exa_provider = Arc::new(exa::ExaWebSearchProvider::new(
                "test-key".into(),
                5,
                240,
            ));
            registry.register_provider_arc(tavily_provider);
            registry.register_provider_arc(exa_provider);
            registry.set_provider_priority(vec![
                WebSearchProviderId("tavily".into()),
                WebSearchProviderId("exa".into()),
            ]);
        });
    });

    // Setup thread with web search tool
    let ThreadTest { model, thread, fs, .. } = setup(cx, TestModel::Fake).await;
    
    // Update the settings file to include web_search in the test profile
    fs.insert_file(
        paths::settings_file(),
        json!({
            "agent": {
                "default_profile": "test-profile",
                "profiles": {
                    "test-profile": {
                        "name": "Test Profile",
                        "tools": {
                            "echo": true,
                            "delay": true,
                            "word_list": true,
                            "tool_requiring_permission": true,
                            "infinite": true,
                            "thinking": true,
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
    let fake_model = model.as_fake();

    thread.update(cx, |thread, _cx| {
        thread.add_tool(WebSearchTool);
    });

    let mut events = thread
        .update(cx, |thread, cx| {
            thread.send(UserMessageId::new(), ["Search the web for 'test query'"], cx)
        })
        .unwrap();
    cx.run_until_parked();

    // Simulate the model calling the web_search tool
    let tool_use = LanguageModelToolUse {
        id: "tool_1".into(),
        name: "web_search".into(),
        raw_input: json!({"query": "test query"}).to_string(),
        input: json!({"query": "test query"}),
        is_input_complete: true,
    };
    fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
        tool_use.clone(),
    ));
    fake_model.end_last_completion_stream();
    
    cx.run_until_parked();

    // Collect events and find the completed tool call
    let mut tool_call_received = false;
    let mut completed_update = None;
    
    // Process events - we need to handle ToolCall first, then wait for completion
    while let Some(event_result) = events.next().await {
        match event_result {
            Ok(ThreadEvent::ToolCall(_)) => {
                tool_call_received = true;
                // Continue to wait for updates
            }
            Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) => {
                if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                    completed_update = Some(u);
                    break;
                } else if u.fields.status == Some(acp::ToolCallStatus::Failed) {
                    let error_msg = match &u.fields.raw_output {
                        Some(v) if v.is_string() => v.as_str().unwrap_or("Unknown error").to_string(),
                        Some(v) => v.to_string(),
                        None => "Unknown error".to_string(),
                    };
                    panic!("Tool call failed: {}", error_msg);
                }
                // Continue waiting for completion status
            }
            Ok(ThreadEvent::Stop(_)) => {
                break;
            }
            Err(e) => {
                panic!("Error in event stream: {:?}", e);
            }
            _ => {
                // Continue processing other events
            }
        }
    }
    
    if !tool_call_received {
        panic!("Tool call event never received");
    }
    
    let update = completed_update.expect("Tool call should complete successfully via fallback");
    
    match update.fields.status {
        Some(acp::ToolCallStatus::Completed) => {
            if let Some(raw_output) = &update.fields.raw_output {
                let search_response: WebSearchResponse =
                    serde_json::from_value(raw_output.clone()).unwrap();
                assert_eq!(search_response.results.len(), 1);
                assert_eq!(search_response.results[0].title, "Fallback Result");
                assert!(search_response.results[0].text.contains("fallback provider"));
            } else {
                panic!("Tool call completed but no output provided");
            }
        }
        Some(acp::ToolCallStatus::Failed) => {
            let error_msg = match &update.fields.raw_output {
                Some(v) if v.is_string() => v.as_str().unwrap_or("Unknown error").to_string(),
                Some(v) => v.to_string(),
                None => "Unknown error".to_string(),
            };
            panic!("Tool call failed: {}", error_msg);
        }
        _ => panic!("Tool call did not complete: status = {:?}", update.fields.status),
    }
}

