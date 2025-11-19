use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream};
use agent_client_protocol as acp;
use agent_settings::AgentSettings;
use anyhow::{Result, anyhow};
use cloud_llm_client::WebSearchResponse;
use gpui::{App, AppContext, Task};
use language_model::{
    LanguageModelProviderId, LanguageModelToolResultContent,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings as _;
use ui::prelude::*;
use web_search::{WebSearchProviderId, WebSearchRegistry};

/// Search the web for information using your query.
/// Use this when you need real-time information, facts, or data that might not be in your training.
/// Results will include snippets and links from relevant web pages.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WebSearchToolInput {
    /// The search term or question to query on the web.
    query: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WebSearchToolOutput(WebSearchResponse);

impl From<WebSearchToolOutput> for LanguageModelToolResultContent {
    fn from(value: WebSearchToolOutput) -> Self {
        serde_json::to_string(&value.0)
            .expect("Failed to serialize WebSearchResponse")
            .into()
    }
}

pub struct WebSearchTool;

impl AgentTool for WebSearchTool {
    type Input = WebSearchToolInput;
    type Output = WebSearchToolOutput;

    fn name() -> &'static str {
        "web_search"
    }

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Fetch
    }

    fn initial_title(
        &self,
        _input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        "Searching the Web".into()
    }

    /// Web search is available if any web search provider is configured.
    fn supports_provider(_provider: &LanguageModelProviderId) -> bool {
        // Web search is now available with multiple providers (Tavily, Exa, Zed)
        // Check is done at runtime based on available providers
        true
    }

    fn run(
        self: Arc<Self>,
        input: Self::Input,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output>> {
        // Collect all needed data first to avoid multiple borrows
        let (preferred_provider_id, max_results, snippet_length) = {
            let agent_settings = AgentSettings::get_global(cx);
            let profile_id = event_stream
                .profile_id()
                .cloned()
                .unwrap_or_else(|| agent_settings.default_profile.clone());
            let profile = agent_settings.profiles.get(&profile_id);

            // Determine preferred provider: profile override -> global default -> priority order
            let preferred = profile
                .and_then(|p| p.web_search_provider.as_ref().map(|s| s.as_str().to_string()))
                .or_else(|| {
                    agent_settings
                        .default_web_search_provider
                        .as_ref()
                        .map(|s| s.as_str().to_string())
                })
                .map(|s| WebSearchProviderId(s.into()));

            let max_results = profile
                .and_then(|p| p.web_search_max_results)
                .unwrap_or(agent_settings.default_web_search_max_results);
            let snippet_length = profile
                .and_then(|p| p.web_search_snippet_length)
                .unwrap_or(agent_settings.default_web_search_snippet_length);

            (preferred, max_results, snippet_length)
        };
        
        let mut providers = {
            let registry = WebSearchRegistry::read_global(cx);
            registry.providers_in_priority_order()
        };
        
        // If a preferred provider is specified, move it to the front
        if let Some(preferred_id) = &preferred_provider_id {
            if let Some(pos) = providers.iter().position(|p| p.id() == *preferred_id) {
                let preferred = providers.remove(pos);
                providers.insert(0, preferred);
            }
        }

        if providers.is_empty() {
            return Task::ready(Err(anyhow!(
                "Web search is not available. No providers configured."
            )));
        }

        let query = input.query.clone();
        // Collect providers and their IDs first
        let provider_data: Vec<(web_search::WebSearchProviderId, Arc<dyn web_search::WebSearchProvider>)> = 
            providers.into_iter().map(|p| (p.id(), p)).collect();
        
        // Spawn search tasks - need to do this sequentially to avoid multiple borrows
        let mut searches = Vec::new();
        for (provider_id, provider) in provider_data {
            let query_clone = query.clone();
            let task = provider.search(query_clone, cx);
            searches.push((provider_id, task));
        }

        cx.background_spawn(async move {
            let mut last_err = None;
            let mut tried = Vec::new();

            for (provider_id, task) in searches {
                tried.push(provider_id.clone());

                let result = task.await;
                match result {
                    Ok(response) => {
                        // Apply runtime trimming based on settings (in case provider defaults differ).
                        let mut response = response;
                        if response.results.len() > max_results {
                            response.results.truncate(max_results);
                        }
                        for result in &mut response.results {
                            if result.text.len() > snippet_length {
                                result.text = truncate_text(&result.text, snippet_length);
                            }
                        }

                        let provider_name = provider_id.0.as_str();
                        event_stream.update_fields(acp::ToolCallUpdateFields {
                            title: Some(format!(
                                "Searched the web using {}: {} results",
                                provider_name,
                                response.results.len()
                            )),
                            ..Default::default()
                        });
                        emit_update(&response, &event_stream);
                        return Ok(WebSearchToolOutput(response));
                    }
                    Err(err) => {
                        let retryable = is_retryable_error(&err);
                        log::warn!(
                            "Web search failed with provider {}: {} (retryable: {})",
                            provider_id.0,
                            err,
                            retryable
                        );
                        last_err = Some(err);
                        if !retryable {
                            break;
                        }
                    }
                }
            }

            let providers_tried = tried
                .iter()
                .map(|id| id.0.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            event_stream.update_fields(acp::ToolCallUpdateFields {
                title: Some(format!("Web Search Failed (tried: {})", providers_tried)),
                ..Default::default()
            });
            Err(last_err.unwrap_or_else(|| anyhow!("Web search failed")))
        })
    }

    fn replay(
        &self,
        _input: Self::Input,
        output: Self::Output,
        event_stream: ToolCallEventStream,
        _cx: &mut App,
    ) -> Result<()> {
        emit_update(&output.0, &event_stream);
        Ok(())
    }
}

fn emit_update(response: &WebSearchResponse, event_stream: &ToolCallEventStream) {
    let result_text = if response.results.len() == 1 {
        "1 result".to_string()
    } else {
        format!("{} results", response.results.len())
    };
    event_stream.update_fields(acp::ToolCallUpdateFields {
        title: Some(format!("Searched the web: {result_text}")),
        content: Some(
            response
                .results
                .iter()
                .map(|result| acp::ToolCallContent::Content {
                    content: acp::ContentBlock::ResourceLink(acp::ResourceLink {
                        name: result.title.clone(),
                        uri: result.url.clone(),
                        title: Some(result.title.clone()),
                        description: Some(result.text.clone()),
                        mime_type: None,
                        annotations: None,
                        size: None,
                        meta: None,
                    }),
                })
                .collect(),
        ),
        ..Default::default()
    });
}

fn is_retryable_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("429")
        || msg.contains("500")
        || msg.contains("502")
        || msg.contains("503")
        || msg.contains("504")
        || msg.contains("timeout")
}

fn truncate_text(text: &str, max_length: usize) -> String {
    if text.len() <= max_length {
        return text.to_string();
    }

    let truncated = &text[..max_length];
    if let Some(last_space) = truncated.rfind(' ') {
        if last_space > max_length / 2 {
            format!("{}...", &truncated[..last_space])
        } else {
            format!("{}...", truncated)
        }
    } else {
        format!("{}...", truncated)
    }
}
