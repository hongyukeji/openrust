use std::sync::Arc;

use openrust::{container::ServiceKey, support::facades::Facade};

use crate::{
    Agent, AiError, AiManager, AiResult, EmbeddingProvider, FakeEmbeddingGateway,
    FakeRerankingGateway, FakeRerankingResult, FakeTextGateway, ModelCatalog, RerankingProvider,
    TextProvider,
    classification::{FakeClassificationGateway, FakeClassificationResult},
};

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

    pub fn build(config: serde_json::Value) -> AiResult<Arc<dyn TextProvider>> {
        Self::manager()?.build(config)
    }

    pub fn flush_state() -> AiResult<()> {
        Self::manager()?.flush_state();
        Ok(())
    }

    pub fn conversation_store() -> AiResult<Arc<dyn crate::ConversationStore>> {
        Self::manager()?
            .conversation_store()
            .map(|(store, _)| store)
    }

    pub fn set_conversation_store(store: Arc<dyn crate::ConversationStore>) -> AiResult<()> {
        Self::manager()?.set_conversation_store(store);
        Ok(())
    }

    pub fn text_provider(name: Option<&str>) -> AiResult<Arc<dyn TextProvider>> {
        Self::manager()?.text_provider(name)
    }

    pub fn fake_embeddings(
        responses: impl IntoIterator<Item = Vec<Vec<f32>>>,
    ) -> AiResult<Arc<FakeEmbeddingGateway>> {
        Ok(Self::manager()?.fake_embeddings(responses))
    }

    pub fn embeddings_are_faked() -> AiResult<bool> {
        Ok(Self::manager()?.embeddings_fake().is_some())
    }

    pub fn fake_reranking(
        responses: impl IntoIterator<Item = FakeRerankingResult>,
    ) -> AiResult<Arc<FakeRerankingGateway>> {
        Ok(Self::manager()?.fake_reranking(responses))
    }

    pub fn reranking_is_faked() -> AiResult<bool> {
        Ok(Self::manager()?.reranking_fake().is_some())
    }

    pub fn fake_audio(
        responses: impl IntoIterator<Item = crate::AudioResponse>,
    ) -> AiResult<Arc<crate::FakeAudioGateway>> {
        Ok(Self::manager()?.fake_audio(responses))
    }

    pub fn fake_audio_with(
        resolve: impl Fn(&crate::RecordedAudio) -> AiResult<crate::AudioResponse>
        + Send
        + Sync
        + 'static,
    ) -> AiResult<Arc<crate::FakeAudioGateway>> {
        Ok(Self::manager()?.fake_audio_with(resolve))
    }

    #[track_caller]
    pub fn assert_audio_generated(
        predicate: impl Fn(&crate::RecordedAudio) -> bool,
    ) -> AiResult<()> {
        crate::Audio::assert_generated(predicate)
    }

    #[track_caller]
    pub fn assert_audio_not_generated(
        predicate: impl Fn(&crate::RecordedAudio) -> bool,
    ) -> AiResult<()> {
        crate::Audio::assert_not_generated(predicate)
    }

    #[track_caller]
    pub fn assert_no_audio_generated() -> AiResult<()> {
        crate::Audio::assert_nothing_generated()
    }

    pub fn audio_is_faked() -> AiResult<bool> {
        Ok(Self::manager()?.audio_fake().is_some())
    }

    pub fn audio_provider(name: Option<&str>) -> AiResult<Arc<dyn crate::AudioProvider>> {
        Self::manager()?.audio_provider(name)
    }

    pub fn fake_transcription(
        responses: impl IntoIterator<Item = crate::FakeTranscriptionResult>,
    ) -> AiResult<Arc<crate::FakeTranscriptionGateway>> {
        Ok(Self::manager()?.fake_transcription(responses))
    }

    pub fn fake_transcription_with(
        resolve: impl Fn(&crate::RecordedTranscription) -> AiResult<crate::FakeTranscriptionResult>
        + Send
        + Sync
        + 'static,
    ) -> AiResult<Arc<crate::FakeTranscriptionGateway>> {
        Ok(Self::manager()?.fake_transcription_with(resolve))
    }

    #[track_caller]
    pub fn assert_transcription_generated(
        predicate: impl Fn(&crate::RecordedTranscription) -> bool,
    ) -> AiResult<()> {
        crate::Transcription::assert_generated(predicate)
    }

    #[track_caller]
    pub fn assert_transcription_not_generated(
        predicate: impl Fn(&crate::RecordedTranscription) -> bool,
    ) -> AiResult<()> {
        crate::Transcription::assert_not_generated(predicate)
    }

    #[track_caller]
    pub fn assert_no_transcription_generated() -> AiResult<()> {
        crate::Transcription::assert_nothing_generated()
    }

    pub fn transcription_is_faked() -> AiResult<bool> {
        Ok(Self::manager()?.transcription_fake().is_some())
    }

    pub fn transcription_provider(
        name: Option<&str>,
    ) -> AiResult<Arc<dyn crate::TranscriptionProvider>> {
        Self::manager()?.transcription_provider(name)
    }

    pub fn image_provider(name: Option<&str>) -> AiResult<Arc<dyn crate::ImageProvider>> {
        Self::manager()?.image_provider(name)
    }

    pub fn file_provider(name: Option<&str>) -> AiResult<Arc<dyn crate::FileProvider>> {
        Self::manager()?.file_provider(name)
    }

    pub fn fakeable_file_provider(name: Option<&str>) -> AiResult<Arc<dyn crate::FileProvider>> {
        Self::manager()?.fakeable_file_provider(name)
    }

    pub fn fake_files(
        responses: impl IntoIterator<Item = bytes::Bytes>,
    ) -> AiResult<Arc<crate::FakeFileGateway>> {
        Ok(Self::manager()?.fake_files(responses))
    }

    pub fn files_are_faked() -> AiResult<bool> {
        Ok(Self::manager()?.files_fake().is_some())
    }

    pub fn store_provider(name: Option<&str>) -> AiResult<Arc<dyn crate::StoreProvider>> {
        Self::manager()?.store_provider(name)
    }

    pub fn fakeable_store_provider(name: Option<&str>) -> AiResult<Arc<dyn crate::StoreProvider>> {
        Self::manager()?.fakeable_store_provider(name)
    }

    pub fn fake_stores(
        responses: impl IntoIterator<Item = String>,
    ) -> AiResult<Arc<crate::FakeStoreGateway>> {
        Ok(Self::manager()?.fake_stores(responses))
    }

    pub fn stores_are_faked() -> AiResult<bool> {
        Ok(Self::manager()?.stores_fake().is_some())
    }

    pub fn fake_classification(
        responses: impl IntoIterator<Item = FakeClassificationResult>,
    ) -> AiResult<Arc<FakeClassificationGateway>> {
        Ok(Self::manager()?.fake_classification(responses))
    }

    pub fn classification_is_faked() -> AiResult<bool> {
        Ok(Self::manager()?.classification_fake().is_some())
    }

    pub fn classification_provider(
        name: Option<&str>,
    ) -> AiResult<Arc<dyn crate::ClassificationProvider>> {
        Self::manager()?.classification_provider(name)
    }

    pub fn reranking_provider(name: Option<&str>) -> AiResult<Arc<dyn RerankingProvider>> {
        Self::manager()?.reranking_provider(name)
    }

    pub fn embedding_provider(name: Option<&str>) -> AiResult<Arc<dyn EmbeddingProvider>> {
        Self::manager()?.embedding_provider(name)
    }

    pub async fn models(name: Option<&str>) -> AiResult<ModelCatalog> {
        Ok(Self::text_provider(name)?.models().await)
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

    pub fn fake_with<A: Agent>(
        resolve: impl Fn(&crate::RecordedPrompt) -> AiResult<crate::FakeTextResult>
        + Send
        + Sync
        + 'static,
    ) -> AiResult<Arc<FakeTextGateway>> {
        Ok(Self::manager()?.fake_with::<A>(resolve))
    }

    pub fn fake_responses<A: Agent>(
        responses: impl IntoIterator<Item = crate::FakeTextResult>,
    ) -> AiResult<Arc<FakeTextGateway>> {
        Ok(Self::manager()?.fake_responses::<A>(responses))
    }
}
