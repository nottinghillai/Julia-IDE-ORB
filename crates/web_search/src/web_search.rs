use std::sync::Arc;

use anyhow::Result;
use cloud_llm_client::WebSearchResponse;
use collections::{HashMap, HashSet};
use gpui::{App, AppContext as _, Context, Entity, Global, SharedString, Task};

pub fn init(cx: &mut App) {
    let registry = cx.new(|_cx| WebSearchRegistry::default());
    cx.set_global(GlobalWebSearchRegistry(registry));
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct WebSearchProviderId(pub SharedString);

pub trait WebSearchProvider {
    fn id(&self) -> WebSearchProviderId;
    fn search(&self, query: String, cx: &mut App) -> Task<Result<WebSearchResponse>>;
}

struct GlobalWebSearchRegistry(Entity<WebSearchRegistry>);

impl Global for GlobalWebSearchRegistry {}

#[derive(Default)]
pub struct WebSearchRegistry {
    providers: HashMap<WebSearchProviderId, Arc<dyn WebSearchProvider>>,
    active_provider: Option<Arc<dyn WebSearchProvider>>,
    provider_priority: Vec<WebSearchProviderId>,
}

impl WebSearchRegistry {
    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalWebSearchRegistry>().0.clone()
    }

    pub fn read_global(cx: &App) -> &Self {
        cx.global::<GlobalWebSearchRegistry>().0.read(cx)
    }

    pub fn providers(&self) -> impl Iterator<Item = &Arc<dyn WebSearchProvider>> {
        self.providers.values()
    }

    pub fn active_provider(&self) -> Option<Arc<dyn WebSearchProvider>> {
        self.active_provider.clone()
    }

    pub fn set_active_provider(&mut self, provider: Arc<dyn WebSearchProvider>) {
        self.active_provider = Some(provider.clone());
        self.providers.insert(provider.id(), provider);
    }

    pub fn register_provider<T: WebSearchProvider + 'static>(
        &mut self,
        provider: T,
        _cx: &mut Context<Self>,
    ) {
        let id = provider.id();
        let provider = Arc::new(provider);
        self.providers.insert(id, provider.clone());
        if self.active_provider.is_none() {
            self.active_provider = Some(provider);
        }
    }

    pub fn register_provider_arc(
        &mut self,
        provider: Arc<dyn WebSearchProvider>,
    ) {
        let id = provider.id();
        self.providers.insert(id.clone(), provider.clone());
        if self.active_provider.is_none() {
            self.active_provider = Some(provider);
        }
    }

    pub fn unregister_provider(&mut self, id: WebSearchProviderId) {
        self.providers.remove(&id);
        self.provider_priority.retain(|pid| pid != &id);
        if self.active_provider.as_ref().map(|provider| provider.id()) == Some(id) {
            self.active_provider = None;
        }
    }

    pub fn set_provider_priority(&mut self, priority: Vec<WebSearchProviderId>) {
        self.provider_priority = priority;
    }

    pub fn provider_priority(&self) -> &[WebSearchProviderId] {
        &self.provider_priority
    }

    pub fn get_provider(&self, id: &WebSearchProviderId) -> Option<Arc<dyn WebSearchProvider>> {
        self.providers.get(id).cloned()
    }

    /// Returns providers in priority order, followed by any remaining providers, de-duplicated.
    pub fn providers_in_priority_order(&self) -> Vec<Arc<dyn WebSearchProvider>> {
        let mut seen: HashSet<WebSearchProviderId> = HashSet::default();
        let mut ordered = Vec::new();

        for provider_id in &self.provider_priority {
            if let Some(provider) = self.providers.get(provider_id) {
                if seen.insert(provider_id.clone()) {
                    ordered.push(provider.clone());
                }
            }
        }

        if let Some(active) = &self.active_provider {
            if seen.insert(active.id()) {
                ordered.push(active.clone());
            }
        }

        for (id, provider) in &self.providers {
            if seen.insert(id.clone()) {
                ordered.push(provider.clone());
            }
        }

        ordered
    }

    /// Selects the first available provider from the priority list, or returns the active provider.
    pub fn select_provider_by_priority(&self) -> Option<Arc<dyn WebSearchProvider>> {
        // First try providers in priority order
        for provider_id in &self.provider_priority {
            if let Some(provider) = self.providers.get(provider_id) {
                return Some(provider.clone());
            }
        }
        // Fall back to active provider if no priority provider is available
        self.active_provider.clone()
    }

}
