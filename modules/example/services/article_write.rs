//! 文章写入的唯一业务入口；HTTP 与数据导入共享校验和事务副作用。

use std::sync::Arc;

use crate::modules::example::events::article::ArticleChanged;
use crate::modules::example::http::requests::article::ArticleInput;
use crate::modules::example::models::article;
use crate::modules::example::services::telemetry::{self, ArticleOperation};
#[cfg(feature = "queue")]
use crate::modules::example::services::welcome::verified_email;
use openrust::database::sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseTransaction};
use openrust::events::Events;
#[cfg(feature = "queue")]
use openrust::notifications::{EmailDelivery, Notification, Notifier};
use openrust::prelude::Db;
use openrust::{Error, Result};

#[derive(Clone)]
pub(crate) struct ArticleWriter {
    db: Arc<Db>,
    events: Arc<Events>,
    #[cfg(feature = "queue")]
    notifier: Arc<Notifier>,
}

impl ArticleWriter {
    pub(crate) fn new(
        db: Arc<Db>,
        events: Arc<Events>,
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
        telemetry::observe(
            ArticleOperation::Create,
            self.db.transaction(move |transaction| {
                Box::pin(async move {
                    writer
                        .create_in_transaction(transaction, owner_id, input)
                        .await
                })
            }),
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
