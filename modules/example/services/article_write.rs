//! 文章写入的唯一业务入口；HTTP 与数据导入共享校验和事务副作用。

use std::sync::{Arc, OnceLock};

use crate::modules::example::events::article::ArticleChanged;
use crate::modules::example::http::requests::article::ArticleInput;
use crate::modules::example::models::article;
use crate::modules::example::services::telemetry::{self, ArticleOperation};
#[cfg(feature = "queue")]
use crate::modules::example::services::welcome::verified_email;
use openrust::database::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
    QuerySelect,
};
use openrust::events::Dispatcher;
use openrust::http::axum::http::StatusCode;
#[cfg(feature = "queue")]
use openrust::notifications::{EmailDelivery, Notification, Notifier};
use openrust::prelude::Db;
use openrust::{Error, Result};

#[derive(Clone)]
pub(crate) struct ArticleWriter {
    db: Arc<Db>,
    events: Arc<Dispatcher>,
    #[cfg(feature = "queue")]
    notifier: Arc<Notifier>,
}

impl ArticleWriter {
    pub(crate) fn new(
        db: Arc<Db>,
        events: Arc<Dispatcher>,
        #[cfg(feature = "queue")] notifier: Arc<Notifier>,
    ) -> Self {
        Self {
            db,
            events,
            #[cfg(feature = "queue")]
            notifier,
        }
    }

    /// 每次调用单独提交；批量导入的前序成功行不会因后续坏行回滚。
    pub(crate) async fn create(
        &self,
        owner_id: i64,
        input: ArticleInput,
    ) -> Result<article::Model> {
        let writer = self.clone();
        let work_db = Arc::clone(&self.db);
        let check_db = Arc::clone(&self.db);
        let inserted = Arc::new(OnceLock::new());
        let written = Arc::clone(&inserted);
        telemetry::observe(
            ArticleOperation::Create,
            self.db.transaction_for_update_checked_context(
                move |context| {
                    Box::pin(async move {
                        if !context.is_from(&work_db) {
                            return Err(invalid_transaction_source());
                        }
                        let model = writer
                            .create_in_transaction(context.transaction(), owner_id, input)
                            .await?;
                        written
                            .set(model.id)
                            .map_err(|_| invalid_transaction_source())?;
                        Ok(model)
                    })
                },
                move |context| {
                    Box::pin(async move {
                        if !context.is_from(&check_db) {
                            return Err(invalid_transaction_source());
                        }
                        let id = inserted
                            .get()
                            .copied()
                            .ok_or_else(invalid_transaction_source)?;
                        // 提交前业务监听器也参与此事务；不可交付已被监听器删除或换归属的文章。
                        let current = article::Entity::find_by_id(id)
                            .filter(article::Column::OwnerId.eq(owner_id))
                            .select_only()
                            .column(article::Column::Id)
                            .into_tuple::<i64>()
                            .one(context.transaction())
                            .await
                            .map_err(Error::internal)?;
                        if current.is_none() {
                            return Err(Error::custom_with_status(
                                StatusCode::CONFLICT,
                                "example_article_changed_before_commit",
                                "文章在提交前已不存在或归属改变",
                            ));
                        }
                        Ok(())
                    })
                },
            ),
        )
        .await
    }

    /// 由现有事务持有者调用；实体、事件和通知一同参与该事务，不在此提交。
    pub(crate) async fn create_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        owner_id: i64,
        input: ArticleInput,
    ) -> Result<article::Model> {
        let input = openrust::validation::validate(input)?;
        let model = article::ActiveModel {
            owner_id: Set(owner_id),
            title: Set(input.title),
            body: Set(input.body),
            ..Default::default()
        }
        .insert(transaction)
        .await
        .map_err(Error::internal)?;
        self.events
            .dispatch_in_transaction(
                transaction,
                &ArticleChanged {
                    owner_id,
                    article_id: model.id,
                    action: "created",
                },
            )
            .await?;
        #[cfg(feature = "queue")]
        {
            let recipient = verified_email(&self.db, transaction, owner_id).await?;
            let notification = Notification::new().email(EmailDelivery::new(
                "OpenRust <hello@example.test>",
                recipient,
                "Article created",
                format!("Your article #{} was created.", model.id),
                None,
            )?);
            self.notifier
                .dispatch_in_transaction(transaction, &notification)
                .await?;
        }
        Ok(model)
    }
}

fn invalid_transaction_source() -> Error {
    Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "example_article_transaction_source_invalid",
        "文章写入与提交前检查必须使用同一来源的事务",
    )
}
