mod api_key;
mod cloud;
pub mod exa;
pub mod tavily;

#[cfg(test)]
mod tests;

use agent_settings::AgentSettings;
use client::Client;
use gpui::{App, AsyncApp, Context, Entity};
use language_model::LanguageModelRegistry;
use std::sync::Arc;
use web_search::{WebSearchProvider, WebSearchProviderId, WebSearchRegistry};
use settings::Settings as _;

pub fn init(client: Arc<Client>, cx: &mut App) {
    let registry = WebSearchRegistry::global(cx);
    registry.update(cx, |registry, cx| {
        register_web_search_providers(registry, client, cx);
    });
}

/// Registers web search providers with fallback support.
/// Providers are registered in priority order: Tavily -> Exa -> Zed
pub async fn register_providers_async(
    registry: Entity<WebSearchRegistry>,
    _client: Arc<Client>,
    max_results: usize,
    snippet_length: usize,
    preferred_provider: Option<String>,
    cx: &mut AsyncApp,
) {
    let mut priority = Vec::new();
    let mut providers_to_register: Vec<(WebSearchProviderId, Arc<dyn WebSearchProvider>)> = Vec::new();

    // Try to register Tavily
    if let Ok(Some(api_key)) = api_key::load_api_key(
        tavily::TAVILY_PROVIDER_ID,
        tavily::TAVILY_API_KEY_ENV_VAR,
        cx,
    )
    .await
    {
        let provider = Arc::new(tavily::TavilyWebSearchProvider::new(
            api_key,
            max_results,
            snippet_length,
        ));
        let id = provider.id();
        priority.push(id.clone());
        providers_to_register.push((id, provider));
    }

    // Try to register Exa
    if let Ok(Some(api_key)) = api_key::load_api_key(
        exa::EXA_PROVIDER_ID,
        exa::EXA_API_KEY_ENV_VAR,
        cx,
    )
    .await
    {
        let provider = Arc::new(exa::ExaWebSearchProvider::new(
            api_key,
            max_results,
            snippet_length,
        ));
        let id = provider.id();
        priority.push(id.clone());
        providers_to_register.push((id, provider));
    }

    // If a preferred provider is configured and registered, move it to the front.
    if let Some(preferred) = preferred_provider.as_ref() {
        if let Some(pos) = priority
            .iter()
            .position(|id| id.0.as_ref() == preferred)
        {
            let preferred_id = priority.remove(pos);
            priority.insert(0, preferred_id);
        }
    }

    // Register providers and set priority
    registry.update(cx, |registry, _cx| {
        for (_id, provider) in providers_to_register {
            registry.register_provider_arc(provider);
        }
        registry.set_provider_priority(priority);
    })
    .ok();
}

fn register_web_search_providers(
    registry: &mut WebSearchRegistry,
    client: Arc<Client>,
    cx: &mut Context<WebSearchRegistry>,
) {
    let agent_settings = AgentSettings::get_global(cx);
    let default_provider = agent_settings
        .default_web_search_provider
        .as_ref()
        .map(|s| s.as_str().to_string());
    let max_results = agent_settings.default_web_search_max_results;
    let snippet_length = agent_settings.default_web_search_snippet_length;

    // Register Zed provider (if available)
    register_zed_web_search_provider(
        registry,
        client.clone(),
        &LanguageModelRegistry::global(cx),
        cx,
    );

    // Register Tavily and Exa providers asynchronously
    let registry_entity = WebSearchRegistry::global(cx);
    let client_clone = client.clone();
    let preferred_provider = default_provider.clone();
    cx.spawn(async move |_this, mut cx| {
        register_providers_async(
            registry_entity,
            client_clone,
            max_results,
            snippet_length,
            preferred_provider,
            &mut cx,
        )
        .await;
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);

    cx.subscribe(
        &LanguageModelRegistry::global(cx),
        move |this, registry, event, cx| {
            if let language_model::Event::DefaultModelChanged = event {
                register_zed_web_search_provider(this, client.clone(), &registry, cx)
            }
        },
    )
    .detach();
}

fn register_zed_web_search_provider(
    registry: &mut WebSearchRegistry,
    client: Arc<Client>,
    language_model_registry: &Entity<LanguageModelRegistry>,
    cx: &mut Context<WebSearchRegistry>,
) {
    let using_zed_provider = language_model_registry
        .read(cx)
        .default_model()
        .is_some_and(|default| default.is_provided_by_zed());
    if using_zed_provider {
        registry.register_provider(cloud::CloudWebSearchProvider::new(client, cx), cx)
    } else {
        registry.unregister_provider(WebSearchProviderId(
            cloud::ZED_WEB_SEARCH_PROVIDER_ID.into(),
        ));
    }
}
