use openrust::{
    console::{Application as Artisan, AsyncCommandCallback, ConsoleError},
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{
            Builder, MassPrunable, Model, ModelMetadata, ModelPruners, ModelState, Observer,
            concerns::{ModelEvent, ModelEventResponse},
        },
    },
    foundation::Application,
    support::ProviderResult,
};

const HARD: &str = "fixture::MassEntry";
const SOFT: &str = "fixture::SoftMassEntry";

struct MassEntry<const SOFT_DELETES: bool>(ModelState);

impl<const SOFT_DELETES: bool> Model for MassEntry<SOFT_DELETES> {
    fn metadata() -> &'static ModelMetadata {
        static HARD: ModelMetadata =
            ModelMetadata::new("fixture_mass_entries").timestamps(None, None);
        static SOFT: ModelMetadata = ModelMetadata::new("fixture_mass_entries")
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        if SOFT_DELETES { &SOFT } else { &HARD }
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

impl<const SOFT_DELETES: bool> MassPrunable for MassEntry<SOFT_DELETES> {
    fn prunable() -> DatabaseResult<Builder<'static, Self>> {
        let mut query =
            Self::query()?.with_global_scope("visible", |query: &mut Builder<'_, Self>| {
                query.r#where("visible", "=", 1_i64);
                Ok(())
            })?;
        query.r#where("eligible", "=", 1_i64);
        query.r#where("kind", "=", i64::from(SOFT_DELETES));
        query = query.order_by("id", "asc");
        if std::env::var_os("FIXTURE_MASS_PRUNE_ZERO_CALLBACK").is_some() {
            let base = query.get_query().before_query(|query| Ok(query.limit(0)));
            return Ok(Self::new_eloquent_builder(base));
        }
        if std::env::var_os("FIXTURE_MASS_PRUNE_ZERO_LIMIT").is_some() {
            return Ok(query.limit(0));
        }
        Ok(if SOFT_DELETES { query } else { query.limit(2) })
    }
}

struct RejectModelEvents;

impl<const SOFT_DELETES: bool> Observer<MassEntry<SOFT_DELETES>> for RejectModelEvents {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Retrieved, ModelEvent::Deleting];

    fn retrieved<'a>(
        &'a self,
        _: &'a mut MassEntry<SOFT_DELETES>,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        panic!("批量清理不应实例化模型");
    }

    fn deleting<'a>(
        &'a self,
        _: &'a mut MassEntry<SOFT_DELETES>,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        panic!("批量清理不应派发逐实例删除事件");
    }
}

pub(super) async fn register(app: &Application) -> ProviderResult {
    if std::env::var_os("FIXTURE_MASS_PRUNE").is_some() {
        let registry = app.make::<ModelPruners>().await?;
        registry.register_mass_prunable::<MassEntry<false>>(HARD, "app/models/mass_entry.rs")?;
        registry
            .register_mass_prunable::<MassEntry<true>>(SOFT, "app/models/soft_mass_entry.rs")?;
        MassEntry::<false>::observe(app, RejectModelEvents)?;
        MassEntry::<true>::observe(app, RejectModelEvents)?;
    }
    Ok(())
}

pub(super) fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.command_with(
        "webhooks:mass-prune",
        AsyncCommandCallback::new(|app, _, output| {
            Box::pin(async move {
                let count = MassEntry::<true>::prune_all(app, 1).await?;
                output.info(&format!("批量清理 {count} 条记录"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
