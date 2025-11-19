use std::sync::Arc;

use anyhow::{Context as _, Result};
use cloud_llm_client::WebSearchResponse;
use futures::AsyncReadExt as _;
use gpui::{App, AppContext, Task};
use http_client::{HttpClient, Method};
use serde::{Deserialize, Serialize};
use web_search::{WebSearchProvider, WebSearchProviderId};

pub const EXA_PROVIDER_ID: &str = "exa";
const EXA_API_URL: &str = "https://api.exa.ai/search";
pub const EXA_API_KEY_ENV_VAR: &str = "EXA_API_KEY";

#[derive(Serialize)]
struct ExaContents {
    text: bool,
    highlights: bool,
}

#[derive(Serialize)]
struct ExaRequest {
    query: String,
    num_results: usize,
    #[serde(rename = "type")]
    search_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    contents: Option<ExaContents>,
}

#[derive(Deserialize)]
struct ExaResponse {
    results: Vec<ExaResult>,
}

#[derive(Deserialize)]
struct ExaResult {
    title: String,
    url: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    highlights: Option<Vec<String>>,
}

pub struct ExaWebSearchProvider {
    api_key: Arc<str>,
    max_results: usize,
    snippet_length: usize,
}

impl ExaWebSearchProvider {
    pub fn new(api_key: Arc<str>, max_results: usize, snippet_length: usize) -> Self {
        Self {
            api_key,
            max_results,
            snippet_length,
        }
    }
}

impl WebSearchProvider for ExaWebSearchProvider {
    fn id(&self) -> WebSearchProviderId {
        WebSearchProviderId(EXA_PROVIDER_ID.into())
    }

    fn search(&self, query: String, cx: &mut App) -> Task<Result<WebSearchResponse>> {
        let api_key = self.api_key.clone();
        let max_results = self.max_results;
        let snippet_length = self.snippet_length;
        let http_client = cx.http_client();

        cx.background_spawn(async move {
            perform_exa_search(http_client, api_key, query, max_results, snippet_length).await
        })
    }
}

async fn perform_exa_search(
    http_client: Arc<dyn HttpClient>,
    api_key: Arc<str>,
    query: String,
    max_results: usize,
    snippet_length: usize,
) -> Result<WebSearchResponse> {
    let request_body = ExaRequest {
        query,
        num_results: max_results,
        search_type: "keyword",
        contents: Some(ExaContents {
            text: true,      // Request text content
            highlights: true, // Request highlights
        }),
    };

    let request = http_client::Request::builder()
        .method(Method::POST)
        .uri(EXA_API_URL)
        .header("Content-Type", "application/json")
        .header("x-api-key", api_key.as_ref())
        .header("Authorization", format!("Bearer {}", api_key.as_ref()))
        .body(serde_json::to_string(&request_body)?.into())?;

    let mut response = http_client
        .send(request)
        .await
        .context("failed to send Exa search request")?;

    if !response.status().is_success() {
        let mut body = String::new();
        response.body_mut().read_to_string(&mut body).await?;
        anyhow::bail!(
            "Exa search failed. Status: {:?}, Body: {}",
            response.status(),
            body
        );
    }

    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    let exa_response: ExaResponse = serde_json::from_str(&body)
        .context("failed to parse Exa response")?;

    let results = exa_response
        .results
        .into_iter()
        .map(|result| {
            let mut text_parts = Vec::new();
            if let Some(text) = result.text {
                text_parts.push(text);
            }
            if let Some(highlights) = result.highlights {
                text_parts.extend(highlights);
            }
            let text = text_parts.join(" ");
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
