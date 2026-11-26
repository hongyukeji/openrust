use std::sync::Arc;

use openrust::{container::ServiceKey, support::facades::Facade};

use crate::{Agent, AiError, AiManager, AiResult, FakeTextGateway, TextProvider};

pub struct Ai;

impl Facade for Ai {
    type Service = AiManager;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<AiManager>()
    }
}

impl Ai {
    pub fn manager() -> AiResult<Arc<AiManager>> {
        Self::get_facade_root().map_err(|_| AiError::Unavailable)
    }

    pub fn text_provider(name: Option<&str>) -> AiResult<Arc<dyn TextProvider>> {
        Self::manager()?.text_provider(name)
    }

    pub fn text_provider_for(
        agent: &impl Agent,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn TextProvider>> {
        Self::manager()?.text_provider_for(agent, name)
    }

    pub fn fake<A: Agent>(
        responses: impl IntoIterator<Item = String>,
    ) -> AiResult<Arc<FakeTextGateway>> {
        Ok(Self::manager()?.fake::<A>(responses))
    }
}
