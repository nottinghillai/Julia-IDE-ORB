use std::sync::Arc;

use anyhow::{Context as _, Result};
use cloud_llm_client::WebSearchResponse;
use futures::AsyncReadExt as _;
use gpui::{App, AppContext, Task};
use http_client::{HttpClient, Method};
use serde::{Deserialize, Serialize};
use web_search::{WebSearchProvider, WebSearchProviderId};

pub const TAVILY_PROVIDER_ID: &str = "tavily";
const TAVILY_API_URL: &str = "https://api.tavily.com/search";
pub const TAVILY_API_KEY_ENV_VAR: &str = "TAVILY_API_KEY";

#[derive(Serialize)]
struct TavilyRequest {
    api_key: String,
    query: String,
    max_results: usize,
    search_depth: &'static str,
}

#[derive(Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

#[derive(Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
}

pub struct TavilyWebSearchProvider {
    api_key: Arc<str>,
    max_results: usize,
    snippet_length: usize,
}

impl TavilyWebSearchProvider {
    pub fn new(api_key: Arc<str>, max_results: usize, snippet_length: usize) -> Self {
        Self {
            api_key,
            max_results,
            snippet_length,
        }
    }
}

impl WebSearchProvider for TavilyWebSearchProvider {
    fn id(&self) -> WebSearchProviderId {
        WebSearchProviderId(TAVILY_PROVIDER_ID.into())
    }

    fn search(&self, query: String, cx: &mut App) -> Task<Result<WebSearchResponse>> {
        let api_key = self.api_key.clone();
        let max_results = self.max_results;
        let snippet_length = self.snippet_length;
        let http_client = cx.http_client();

        cx.background_spawn(async move {
            perform_tavily_search(http_client, api_key, query, max_results, snippet_length).await
        })
    }
}

async fn perform_tavily_search(
    http_client: Arc<dyn HttpClient>,
    api_key: Arc<str>,
    query: String,
    max_results: usize,
    snippet_length: usize,
) -> Result<WebSearchResponse> {
    let request_body = TavilyRequest {
        api_key: api_key.to_string(),
        query,
        max_results,
        search_depth: "basic",
    };

    let request = http_client::Request::builder()
        .method(Method::POST)
        .uri(TAVILY_API_URL)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&request_body)?.into())?;

    let mut response = http_client
        .send(request)
        .await
        .context("failed to send Tavily search request")?;

    if !response.status().is_success() {
        let mut body = String::new();
        response.body_mut().read_to_string(&mut body).await?;
        anyhow::bail!(
            "Tavily search failed. Status: {:?}, Body: {}",
            response.status(),
            body
        );
    }

    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    let tavily_response: TavilyResponse = serde_json::from_str(&body)
        .context("failed to parse Tavily response")?;

    let results = tavily_response
        .results
        .into_iter()
        .map(|result| {
            let text = result
                .content
                .or(result.snippet)
                .unwrap_or_default();
            let text = strip_html(&text);
            let text = truncate_text(&text, snippet_length);

            cloud_llm_client::WebSearchResult {
                title: result.title,
                url: result.url,
                text,
            }
        })
        .collect();

    Ok(WebSearchResponse { results })
}

pub(crate) fn strip_html(text: &str) -> String {
    // Simple HTML tag removal - could be enhanced with a proper HTML parser
    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

pub(crate) fn truncate_text(text: &str, max_length: usize) -> String {
    if text.len() <= max_length {
        return text.to_string();
    }

    // Try to truncate at word boundary
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
