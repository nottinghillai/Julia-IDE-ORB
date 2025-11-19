use super::*;
use crate::{tools::WebSearchTool, AgentMessageContent, Message};
use acp_thread::UserMessageId;
use cloud_llm_client::WebSearchResponse;
use futures::StreamExt;
use gpui::{http_client::FakeHttpClient, TestAppContext};
use http_client::{AsyncBody, Request, Response};
use language_model::{LanguageModelCompletionEvent, LanguageModelToolUse};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use web_search::{WebSearchProviderId, WebSearchRegistry};
use web_search_providers::tavily;

/// End-to-end test: Verify that the LLM actually calls web_search tool when prompted
/// and receives results back
#[gpui::test]
async fn test_llm_calls_web_search_tool_and_receives_results(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    
    // Track if web search was actually called
    let search_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let search_called_clone = search_called.clone();
    
    // Create a fake HTTP client that captures web search requests
    let http_client = FakeHttpClient::create(move |req: Request<AsyncBody>| {
        let search_called = search_called_clone.clone();
        async move {
            // Mark that web search was called
            if req.uri().to_string().contains("tavily.com") || req.uri().to_string().contains("exa.ai") {
                search_called.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            
            // Return mock response for Tavily
            if req.uri().to_string() == "https://api.tavily.com/search" {
                let response_body = json!({
                    "results": [
                        {
                            "title": "Rust Programming Language",
                            "url": "https://www.rust-lang.org/",
                            "content": "Rust is a systems programming language that runs blazingly fast, prevents segfaults, and guarantees thread safety."
                        },
                        {
                            "title": "Rust Documentation",
                            "url": "https://doc.rust-lang.org/",
                            "snippet": "The Rust Programming Language book and documentation"
                        }
                    ]
                });
                
                return Ok(Response::builder()
                    .status(200)
                    .body(serde_json::to_string(&response_body)?.into())
                    .unwrap());
            }
            
            // Return mock response for Exa (fallback)
            if req.uri().to_string() == "https://api.exa.ai/search_and_contents" {
                let response_body = json!({
                    "results": [
                        {
                            "title": "Rust Programming Language",
                            "url": "https://www.rust-lang.org/",
                            "text": "Rust is a systems programming language",
                            "highlights": ["blazingly fast", "prevents segfaults"]
                        }
                    ]
                });
                
                return Ok(Response::builder()
                    .status(200)
                    .body(serde_json::to_string(&response_body)?.into())
                    .unwrap());
            }
            
            // Default response
            Ok(Response::builder()
                .status(404)
                .body("Not found".into())
                .unwrap())
        }
    });
    
    // Initialize web search registry
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

    // Setup thread
    let ThreadTest { model, thread, fs, .. } = setup(cx, TestModel::Fake).await;
    let fake_model = model.as_fake();
    
    // Enable web_search in profile
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

    thread.update(cx, |thread, _cx| {
        thread.add_tool(WebSearchTool);
    });

    // Send a prompt that should trigger web search
    let mut events = thread
        .update(cx, |thread, cx| {
            thread.send(
                UserMessageId::new(),
                ["What is Rust programming language? Search the web for current information about Rust."],
                cx
            )
        })
        .unwrap();
    cx.run_until_parked();

    // Check that web_search tool is available to the LLM
    let completion = fake_model.pending_completions().pop().unwrap();
    let tool_names: Vec<String> = completion
        .tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    
    assert!(
        tool_names.contains(&"web_search".to_string()),
        "web_search tool should be available to LLM. Available tools: {:?}",
        tool_names
    );

    // Simulate LLM deciding to call web_search tool
    let tool_use = LanguageModelToolUse {
        id: "tool_1".into(),
        name: "web_search".into(),
        raw_input: json!({"query": "Rust programming language"}).to_string(),
        input: json!({"query": "Rust programming language"}),
        is_input_complete: true,
    };
    
    fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
        tool_use.clone(),
    ));
    fake_model.end_last_completion_stream();
    cx.run_until_parked();

    // Wait for tool call to complete
    let mut tool_call_completed = false;
    let mut tool_result_received = false;
    
    while let Some(event_result) = events.next().await {
        match event_result {
            Ok(ThreadEvent::ToolCall(_)) => {
                // Tool call started
            }
            Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) => {
                if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                    tool_call_completed = true;
                    
                    // Verify we got search results
                    if let Some(raw_output) = &u.fields.raw_output {
                        let search_response: WebSearchResponse =
                            serde_json::from_value(raw_output.clone()).unwrap();
                        
                        assert!(!search_response.results.is_empty(), "Search should return results");
                        assert_eq!(search_response.results[0].title, "Rust Programming Language");
                        tool_result_received = true;
                    }
                    break;
                }
            }
            Ok(ThreadEvent::Stop(_)) => break,
            Err(e) => panic!("Error in event stream: {:?}", e),
            _ => {}
        }
    }

    // Verify web search was actually called
    assert!(
        search_called.load(std::sync::atomic::Ordering::SeqCst),
        "Web search HTTP request should have been made"
    );
    
    assert!(
        tool_call_completed,
        "Tool call should complete successfully"
    );
    
    assert!(
        tool_result_received,
        "Tool should return search results"
    );

    // Now simulate LLM receiving the tool result and generating a response
    // Check that the tool result is in the next completion request
    let next_completion = fake_model.pending_completions().pop();
    if let Some(completion) = next_completion {
        // The completion should include the tool result in the messages
        let has_tool_result = completion.messages.iter().any(|msg| {
            matches!(
                msg.content.first(),
                Some(language_model::MessageContent::ToolResult(_))
            )
        });
        
        assert!(
            has_tool_result,
            "LLM should receive tool result in next completion request"
        );
    }

    // Simulate LLM generating final response using the search results
    fake_model.send_last_completion_stream_text_chunk("Based on the search results, Rust is a systems programming language that runs blazingly fast, prevents segfaults, and guarantees thread safety.");
    fake_model
        .send_last_completion_stream_event(LanguageModelCompletionEvent::Stop(
            language_model::StopReason::EndTurn,
        ));
    fake_model.end_last_completion_stream();
    cx.run_until_parked();

    // Verify the final message contains information from search results
    thread.read_with(cx, |thread, _cx| {
        let last_message = thread.last_message();
        if let Some(Message::Agent(agent_msg)) = last_message {
            let text_content: String = agent_msg
                .content
                .iter()
                .filter_map(|c| {
                    if let AgentMessageContent::Text(text) = c {
                        Some(text.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            
            assert!(
                text_content.contains("Rust") || text_content.contains("systems programming"),
                "Final response should contain information from search results. Got: {}",
                text_content
            );
        }
    });
}

/// Test that LLM can use web search results in its response
#[gpui::test]
async fn test_llm_uses_web_search_results_in_response(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    
    // Create HTTP client that returns specific search results
    let http_client = FakeHttpClient::create(|req: Request<AsyncBody>| {
        async move {
            if req.uri().to_string() == "https://api.tavily.com/search" {
                let response_body = json!({
                    "results": [
                        {
                            "title": "Current Weather Information",
                            "url": "https://weather.example.com",
                            "content": "Today's weather is sunny with a temperature of 72°F"
                        }
                    ]
                });
                
                return Ok(Response::builder()
                    .status(200)
                    .body(serde_json::to_string(&response_body)?.into())
                    .unwrap());
            }
            
            Ok(Response::builder()
                .status(404)
                .body("Not found".into())
                .unwrap())
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

    // Prompt that requires web search
    let mut events = thread
        .update(cx, |thread, cx| {
            thread.send(
                UserMessageId::new(),
                ["What's the weather today? Please search the web for current weather information."],
                cx
            )
        })
        .unwrap();
    cx.run_until_parked();

    // Verify tool is available
    let completion = fake_model.pending_completions().pop().unwrap();
    assert!(
        completion.tools.iter().any(|t| t.name == "web_search"),
        "web_search tool should be available"
    );

    // LLM calls web_search
    fake_model.send_last_completion_stream_event(LanguageModelCompletionEvent::ToolUse(
        LanguageModelToolUse {
            id: "tool_1".into(),
            name: "web_search".into(),
            raw_input: json!({"query": "current weather today"}).to_string(),
            input: json!({"query": "current weather today"}),
            is_input_complete: true,
        },
    ));
    fake_model.end_last_completion_stream();
    cx.run_until_parked();

    // Wait for tool completion
    let mut tool_completed = false;
    while let Some(event_result) = events.next().await {
        if let Ok(ThreadEvent::ToolCallUpdate(acp_thread::ToolCallUpdate::UpdateFields(u))) = event_result {
            if u.fields.status == Some(acp::ToolCallStatus::Completed) {
                tool_completed = true;
                
                // Verify search results contain weather info
                if let Some(raw_output) = &u.fields.raw_output {
                    let search_response: WebSearchResponse =
                        serde_json::from_value(raw_output.clone()).unwrap();
                    assert!(search_response.results.iter().any(|r| 
                        r.text.contains("weather") || r.text.contains("temperature")
                    ));
                }
                break;
            }
        }
    }

    assert!(tool_completed, "Tool call should complete");
    
    // Verify next completion includes tool result
    let next_completion = fake_model.pending_completions().pop();
    assert!(
        next_completion.is_some(),
        "LLM should receive tool result and generate next completion"
    );
}

