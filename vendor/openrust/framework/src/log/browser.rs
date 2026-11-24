//! 浏览器日志只在所属请求内收集，正文流结束时输出。
use super::{Level, LogRecord, LogResult, RecordHandler};
use crate::{
    container::Container,
    contracts::container::Container as _,
    http::{Body, Response},
};
use axum::body::Bytes;
use http_body::{Frame, SizeHint};
use serde_json::{Value, json};
use std::{
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
#[derive(Default)]
pub(crate) struct Control(pub AtomicBool);
#[derive(Default)]
pub(crate) struct Buffer(Mutex<Vec<Value>>);
tokio::task_local! {static STREAM: (Weak<Container>,Arc<Buffer>);}
/// 将当前请求的日志输出到浏览器控制台，不向CLI输出脚本。
pub struct BrowserConsoleHandler {
    owner: Weak<Container>,
}
impl BrowserConsoleHandler {
    pub fn new() -> LogResult<Self> {
        let owner = crate::support::facades::current_container()
            .ok_or(super::LogError::InvalidConfiguration)?;
        Self::with_owner(Arc::downgrade(&owner))
    }
    pub(crate) fn with_owner(owner: Weak<Container>) -> LogResult<Self> {
        let app = owner
            .upgrade()
            .ok_or(super::LogError::InvalidConfiguration)?;
        app.make::<Control>()?.0.store(true, Ordering::Release);
        Ok(Self { owner })
    }
    pub fn handle(&self, record: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        let Some(owner) = self.owner.upgrade() else {
            return Ok(());
        };
        let streamed = STREAM
            .try_with(|(app, buffer)| {
                if app.ptr_eq(&self.owner) {
                    Some(buffer.clone())
                } else {
                    None
                }
            })
            .ok()
            .flatten();
        let buffer = if let Some(buffer) = streamed {
            buffer
        } else {
            if crate::http::request_context::identity(&owner).is_none() {
                return Ok(());
            }
            owner.make::<Buffer>()?
        };
        let (context, extra) = super::json::normalized_fields(record);
        let message = if formatted.is_empty() {
            format!(
                "[[{}]]{{macro: autolabel}} [[{}]]{{font-weight: bold}} {}",
                record.channel,
                record.level.label(),
                record.message()
            )
        } else {
            formatted.to_owned()
        };
        let method = match record.level {
            Level::Debug => "debug",
            Level::Info | Level::Notice => "info",
            Level::Warning => "warn",
            _ => "error",
        };
        buffer
            .0
            .lock()
            .expect("浏览器日志锁中毒")
            .push(json!({"method":method,"message":message,"context":context,"extra":extra}));
        Ok(())
    }
}
impl super::FormattableHandler for BrowserConsoleHandler {
    fn get_default_formatter(&self) -> LogResult<Arc<super::Formatter>> {
        Ok(default_formatter())
    }
    fn write(&self, record: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        self.handle(record, formatted)
    }
}
pub(super) fn default_formatter() -> Arc<super::Formatter> {
    Arc::new(|record| {
        Ok(format!(
            "[[{}]]{{macro: autolabel}} [[{}]]{{font-weight: bold}} {}",
            record.channel,
            record.level.label(),
            record.message()
        ))
    })
}
pub(super) fn writer(owner: Weak<Container>) -> LogResult<Arc<RecordHandler>> {
    let handler = BrowserConsoleHandler::with_owner(owner)?;
    Ok(Arc::new(move |record, formatted| {
        handler.handle(record, formatted)
    }))
}
fn script(buffer: &Buffer, html: bool) -> Bytes {
    let records = std::mem::take(&mut *buffer.0.lock().expect("浏览器日志锁中毒"));
    if records.is_empty() {
        return Bytes::new();
    }
    let data = serde_json::to_string(&records)
        .expect("日志值可以序列化")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    // 格式串和数据分离，避免用户消息被控制台当作额外格式参数解释。
    let js = format!(
        r#"(function(c,records){{if(!c||!c.groupCollapsed)return;const labels=new Map(),colors=['blue','green','red','magenta','orange','black','grey'];for(const r of records){{const styles=['font-weight: normal'];let format='%c'+r.message.replace(/\[\[([\s\S]*?)\]\]\{{([^}}]*)\}}/g,(_,text,style)=>{{style=style.replace(/macro\s*:(.*?)(?:;|$)/g,(_,macro)=>{{if(macro.trim()!=='autolabel')return macro;if(!labels.has(text))labels.set(text,colors[labels.size%colors.length]);return 'background-color: '+labels.get(text)+'; color: white; border-radius: 3px; padding: 0 2px 0 2px';}});styles.push(style,'font-weight: normal');return '%c'+text+'%c';}});const groups=[['Context',r.context],['Extra',r.extra]].map(([name,values])=>[name,Object.entries(values).filter(([,v])=>v!==null)]).filter(([,values])=>values.length);c[groups.length?'groupCollapsed':r.method](format,...styles);for(const [name,values] of groups){{c.log('%c%s','font-weight: bold',name);for(const [key,value] of values)c.log('%s: %o',key,value);}}if(groups.length)c.groupEnd();}}}})(globalThis.console,{data});"#
    );
    if html {
        Bytes::from(format!("<script>{js}</script>"))
    } else {
        Bytes::from(js)
    }
}
pub(crate) fn append(
    owner: &Arc<Container>,
    control: &Control,
    response: &mut Response,
    head: bool,
) {
    if head
        || !control.0.load(Ordering::Acquire)
        || !Response::allows_body(response.status())
        || response.is_binary_file()
    {
        return;
    }
    let headers = &response.parts().headers;
    if headers.contains_key(::http::header::CONTENT_RANGE)
        || headers
            .get(::http::header::CONTENT_ENCODING)
            .is_some_and(|v| v != "identity")
    {
        return;
    }
    let kind = headers
        .get(::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_ascii_lowercase();
    let html = kind.contains("text/html");
    if !html && !kind.contains("application/javascript") && !kind.contains("text/javascript") {
        return;
    }
    let Ok(buffer) = owner.make::<Buffer>() else {
        return;
    };
    let body = response.take_body();
    response
        .parts_mut()
        .headers
        .remove(::http::header::CONTENT_LENGTH);
    response.parts_mut().headers.remove(::http::header::ETAG);
    response.replace_body(Body::new(BrowserBody {
        body,
        buffer,
        owner: Arc::downgrade(owner),
        html,
        pending: None,
        finished: false,
    }));
}
struct BrowserBody {
    body: Body,
    buffer: Arc<Buffer>,
    owner: Weak<Container>,
    html: bool,
    pending: Option<Frame<Bytes>>,
    finished: bool,
}
impl http_body::Body for BrowserBody {
    type Data = Bytes;
    type Error = axum::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if let Some(frame) = this.pending.take() {
            return Poll::Ready(Some(Ok(frame)));
        }
        if this.finished {
            return Poll::Ready(None);
        }
        let frame = STREAM.sync_scope((this.owner.clone(), this.buffer.clone()), || {
            Pin::new(&mut this.body).poll_frame(cx)
        });
        match frame {
            Poll::Ready(None) => {
                this.finished = true;
                let suffix = script(&this.buffer, this.html);
                if suffix.is_empty() {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Ok(Frame::data(suffix))))
                }
            }
            Poll::Ready(Some(Ok(frame))) if frame.is_trailers() => {
                this.finished = true;
                let suffix = script(&this.buffer, this.html);
                if suffix.is_empty() {
                    Poll::Ready(Some(Ok(frame)))
                } else {
                    this.pending = Some(frame);
                    Poll::Ready(Some(Ok(Frame::data(suffix))))
                }
            }
            Poll::Ready(Some(Err(error))) => {
                this.finished = true;
                Poll::Ready(Some(Err(error)))
            }
            other => other,
        }
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}
