//! 文章业务复用既有 tracing 与 metrics 出口；标签只允许固定操作与结果。

use openrust::{Error, Result};
use std::{future::Future, time::Instant};
use tracing::Instrument as _;

/// 在事务外观察完整操作，不把尚未提交的写入记为成功。
#[derive(Clone, Copy)]
pub(crate) enum ArticleOperation {
    Create,
    Update,
    Delete,
}
impl ArticleOperation {
    const fn label(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

/// 调用方应将最终提交纳入 future；不用于单独观察事务内的一次 SQL 写入。
///
/// 已配置的框架 subscriber/OTel 桥接负责 span 出口，本模块不安装全局 collector。
/// 外部取消只表示本次观察未完成，不推断数据库或外部系统已撤销副作用。
pub(crate) async fn observe<T>(
    operation: ArticleOperation,
    action: impl Future<Output = Result<T>>,
) -> Result<T> {
    let span = tracing::info_span!("example.article", operation = operation.label());
    let mut observation = Observation {
        operation,
        started: Instant::now(),
        span: span.clone(),
        finished: false,
    };
    let result = action.instrument(span).await;
    observation.finish(match &result {
        Ok(_) => "success",
        Err(error) => classify(error),
    });
    result
}

fn classify(error: &Error) -> &'static str {
    // 不采用可由业务动态构造的 code()/Display，更不把错误详情变成 metric 标签。
    match error.status().as_u16() {
        400 | 422 => "invalid",
        401 | 403 => "denied",
        404 => "not_found",
        409 => "conflict",
        429 => "throttled",
        _ => "failed",
    }
}

struct Observation {
    operation: ArticleOperation,
    started: Instant,
    span: tracing::Span,
    finished: bool,
}
impl Observation {
    fn finish(&mut self, outcome: &'static str) {
        self.finished = true;
        let elapsed = self.started.elapsed().as_secs_f64();
        let operation = self.operation.label();
        self.span.in_scope(|| {
            tracing::info!(
                operation,
                outcome,
                duration_seconds = elapsed,
                "article operation finished"
            );
        });
        #[cfg(feature = "metrics")]
        {
            metrics::counter!("example_article_operations_total", "operation" => operation, "outcome" => outcome).increment(1);
            metrics::histogram!("example_article_operation_duration_seconds", "operation" => operation, "outcome" => outcome).record(elapsed);
        }
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("cancelled");
        }
    }
}
