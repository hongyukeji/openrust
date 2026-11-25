use std::{io::Write as _, path::Path};

use openrust::{
    console::{Application as Artisan, AsyncCommandCallback, ConsoleError},
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseResult, Value,
        eloquent::{
            Builder, Model, ModelMetadata, ModelPruners, ModelState, Observer, Prunable,
            concerns::{ModelEvent, ModelEventResponse},
        },
    },
    foundation::{Application, configuration::ReportDecision},
    support::{ProviderResult, facades::Exceptions},
};

const MODEL: &str = "fixture::CleanupEntry";

struct CleanupEntry<const SOFT: bool = true>(ModelState);

impl<const SOFT: bool> Model for CleanupEntry<SOFT> {
    fn metadata() -> &'static ModelMetadata {
        static HARD: ModelMetadata =
            ModelMetadata::new("fixture_cleanup_entries").timestamps(None, None);
        static SOFT_DELETE: ModelMetadata = ModelMetadata::new("fixture_cleanup_entries")
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        if SOFT { &SOFT_DELETE } else { &HARD }
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

fn record(path: &Path, entry: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{entry}")
}

fn id(model: &impl Model) -> i64 {
    match model.get_key() {
        Some(Value::Integer(id)) => *id,
        value => panic!("意外的清理夹具主键 {value:?}"),
    }
}

impl<const SOFT: bool> Prunable for CleanupEntry<SOFT> {
    fn prunable() -> DatabaseResult<Builder<'static, Self>> {
        let mut query = Self::query()?;
        query.r#where("eligible", "=", 1_i64);
        Ok(query)
    }

    async fn pruning(&mut self) -> DatabaseResult<()> {
        let path = std::env::var_os("FIXTURE_INSTANCE_PRUNE_LOG").expect("清理夹具必须指定日志");
        let id = id(self);
        record(Path::new(&path), &format!("hook {id}")).expect("写入钩子日志");
        if id == 1 {
            return Err(DatabaseError::InvalidQuery("fixture-pruning-failure"));
        }
        Ok(())
    }

    fn prunable_chunk_size(_: u32) -> u32 {
        1
    }
}

struct CleanupObserver(std::path::PathBuf);

impl Observer<CleanupEntry> for CleanupObserver {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::ForceDeleting, ModelEvent::ForceDeleted];

    fn force_deleting<'a>(
        &'a self,
        model: &'a mut CleanupEntry,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if id(model) == 6 {
                record(&self.0, "veto 6").expect("写入拒绝删除日志");
                Ok(ModelEventResponse::Cancel)
            } else {
                Ok(ModelEventResponse::Continue)
            }
        })
    }

    fn force_deleted<'a>(
        &'a self,
        model: &'a mut CleanupEntry,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            record(&self.0, &format!("deleted {}", id(model))).expect("写入删除事件日志");
            Ok(ModelEventResponse::Continue)
        })
    }
}

pub(super) async fn register(app: &Application) -> ProviderResult {
    if let Some(path) = std::env::var_os("FIXTURE_INSTANCE_PRUNE_LOG") {
        let path = std::path::PathBuf::from(path);
        app.make::<ModelPruners>()
            .await?
            .register_prunable::<CleanupEntry>(MODEL, "app/models/cleanup_entry.rs")?;
        CleanupEntry::observe(app, CleanupObserver(path.clone()))?;
        Exceptions::reportable::<DatabaseError>(move |error| {
            if matches!(
                error,
                DatabaseError::InvalidQuery("fixture-pruning-failure")
            ) {
                record(&path, "reported 1").expect("写入错误报告日志");
                return ReportDecision::Stop;
            }
            ReportDecision::Continue
        })?;
    }
    Ok(())
}

pub(super) fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.command_with(
        "webhooks:prune-instance {--plain}",
        AsyncCommandCallback::new(|_, input, _| {
            Box::pin(async {
                let removed = if input.option("plain") == Some(serde_json::json!(true)) {
                    prune_one::<CleanupEntry<false>>(5).await?
                } else {
                    prune_one::<CleanupEntry>(4).await?
                };
                assert!(removed);
                Ok(0)
            })
        }),
    )?;
    artisan.command_with(
        "webhooks:prune-instances",
        AsyncCommandCallback::new(|app, _, output| {
            Box::pin(async {
                let count = CleanupEntry::<true>::prune_all(app, 10).await?;
                output.info(&format!("清理计数 {count}"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

async fn prune_one<M: Prunable>(id: i64) -> Result<bool, ConsoleError> {
    let mut record = M::find_or_fail(id)
        .await
        .map_err(ConsoleError::from_source)?;
    record.prune().await.map_err(ConsoleError::from_source)
}
