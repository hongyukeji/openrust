mod native;
use crate::{IncomingEntry, Telescope, facades};
use openrust::{
    contracts::{
        config::Repository,
        events::{Event, Listener},
    },
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::http::events::RequestHandled,
    support::{facades::Facade, php_loose_string_value_equal},
};
use serde_json::{Map, Value, json};
use std::sync::Arc;

mod watcher;
pub use watcher::*;

struct EventListener {
    watcher: NativeWatcher,
}
impl Listener for EventListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let options = &self.watcher;
            if !options.enabled("EventWatcher")
                || options.patterns("EventWatcher", "ignore", event.name())
            {
                return Ok(None);
            }
            let Ok(telescope) = facades::Telescope::get_facade_root() else {
                return Ok(None);
            };
            if !telescope.automatic_recording() || !telescope.has_started() {
                return Ok(None);
            }
            if telescope
                .ignore_framework_events
                .load(std::sync::atomic::Ordering::Acquire)
                && (event.type_name().starts_with("openrust::")
                    || matches(
                        &json!([
                            "OpenRust\\*",
                            "Illuminate\\*",
                            "eloquent*",
                            "bootstrapped*",
                            "bootstrapping*",
                            "creating*",
                            "composing*"
                        ]),
                        event.name(),
                    ))
            {
                return Ok(None);
            }
            telescope.initialize().await;
            if !telescope.is_recording() {
                return Ok(None);
            }
            if telescope.protect_sensitive_request().is_some() {
                return Ok(None);
            }
            let dispatcher = openrust::support::facades::Event::get_facade_root()
                .map_err(openrust::events::EventError::Unavailable)?;
            let listeners = dispatcher
                .get_listeners(event.name())
                .into_iter()
                .filter(|(name, _)| !name.contains("openrust_telescope::"))
                .map(|(name, queued)| json!({"name":name,"queued":queued}))
                .collect::<Vec<_>>();
            let payload = event.properties().or_else(|| {
                context.arguments_for(event).map(|arguments| {
                    arguments
                        .iter()
                        .map(|argument| native::format_argument(*argument))
                        .collect::<Vec<_>>()
                        .into()
                })
            });
            telescope.record_event(IncomingEntry::make(json!({"name":event.name(),"payload":payload,"listeners":listeners,"broadcast":dispatcher.is_broadcasting(event)}).as_object().unwrap().clone()).map_err(|source|openrust::events::EventError::Listener{source:Box::new(source)})?);
            Ok(None)
        })
    }
}
fn register_logging(
    manager: &openrust::log::LogManager,
    watcher: NativeWatcher,
) -> openrust::log::LogResult<()> {
    manager.listen(move |event| {
        let Ok(telescope) = facades::Telescope::get_facade_root() else {
            return Ok(());
        };
        if telescope.automatic_recording() && telescope.has_started() && telescope.is_recording() {
            watcher
                .record(event, &telescope, None)
                .map_err(|source| openrust::log::LogError::WriteFailed(Box::new(source)))?;
        }
        Ok(())
    })
}
#[derive(Clone)]
struct NativeWatcher {
    config: Arc<dyn Repository>,
    only: Option<(&'static str, Map<String, Value>)>,
}

fn register_views(
    factory: &openrust::view::Factory,
    watcher: NativeWatcher,
    base: std::path::PathBuf,
) {
    factory.composer("*", move |view| {
        use openrust::view::ViewFinder as _;
        let Ok(telescope) = facades::Telescope::get_facade_root() else {
            return Ok(());
        };
        if !watcher.enabled("ViewWatcher")
            || !telescope.automatic_recording()
            || !telescope.is_recording()
        {
            return Ok(());
        }
        let path = openrust::support::facades::View::get_facade_root()
            .ok()
            .and_then(|factory| factory.get_finder().find(view.name()).ok())
            .map(|path| {
                path.strip_prefix(&base)
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or(path)
            });
        let keys = view
            .data()
            .keys()
            .filter(|key| !["app", "__env", "obLevel", "errors"].contains(&key.as_str()))
            .collect::<Vec<_>>();
        let entry = IncomingEntry::make(
            json!({"name":view.name(),"path":path,"data":keys,"composers":null})
                .as_object()
                .unwrap()
                .clone(),
        )
        .map_err(|source| openrust::view::ViewError::RenderFailed(Box::new(source)))?;
        telescope.record_view(entry);
        Ok(())
    });
}
impl NativeWatcher {
    fn option(&self, watcher: &str, key: &str) -> Option<Value> {
        if let Some((name, options)) = &self.only {
            return (*name == watcher)
                .then(|| options.get(key).cloned())
                .flatten();
        }
        self.config
            .get(&format!("telescope.watchers.{watcher}.{key}"))
    }
    fn enabled(&self, watcher: &str) -> bool {
        if let Some((name, options)) = &self.only {
            return *name == watcher
                && options
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
        }
        match self.config.get(&format!("telescope.watchers.{watcher}")) {
            Some(Value::Bool(enabled)) => enabled,
            Some(Value::Object(options)) => options
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            _ => false,
        }
    }
    fn patterns(&self, watcher: &str, key: &str, value: &str) -> bool {
        matches(&self.option(watcher, key).unwrap_or(json!([])), value)
    }
    fn cache_ignored(&self, key: &str) -> bool {
        self.patterns("CacheWatcher", "ignore", key)
            || matches(
                &json!([
                    "illuminate:queue:restart",
                    "framework/schedule*",
                    "laravel:reverb:restart",
                    "telescope:*",
                    "openrust:queue:restart",
                    "openrust:reverb:restart"
                ]),
                key,
            )
    }
    fn cache_value(&self, key: &str, value: &openrust::cache::Value) -> Value {
        if self.patterns("CacheWatcher", "hidden", key) {
            return "********".into();
        }
        match value {
            openrust::cache::Value::Json(value) => value.clone(),
            openrust::cache::Value::Bytes(bytes) => {
                String::from_utf8_lossy(bytes).into_owned().into()
            }
        }
    }
    fn approved_path(&self, path: &str, host: Option<&str>) -> bool {
        if let Some(Value::String(domain)) = self.config.get("telescope.domain")
            && host == Some(domain.as_str())
        {
            return false;
        }
        let path = path.trim_start_matches('/');
        if let Some(only @ Value::Array(_)) = self.config.get("telescope.only_paths")
            && only.as_array().is_some_and(|paths| !paths.is_empty())
        {
            return matches(&only, path);
        }
        let prefix = self
            .config
            .get("telescope.path")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("telescope".into());
        let horizon = self
            .config
            .get("horizon.path")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("horizon".into());
        if matches(
            &json!([
                format!("{prefix}*"),
                format!("{horizon}*"),
                "telescope-api*",
                "vendor/telescope*",
                "vendor/horizon*"
            ]),
            path,
        ) {
            return false;
        }
        !matches(
            &self
                .config
                .get("telescope.ignore_paths")
                .unwrap_or(json!([])),
            path,
        )
    }
    fn response_content(&self, telescope: &Telescope, event: &RequestHandled) -> Value {
        let content = event.response_content.as_deref();
        if let Some(bytes) = content {
            let text = String::from_utf8_lossy(bytes);
            let limit = self
                .option("RequestWatcher", "size_limit")
                .and_then(|value| value.as_u64())
                .unwrap_or(64);
            let within = (text.chars().count() as u64) / 1000 <= limit;
            if let Ok(value @ (Value::Array(_) | Value::Object(_))) =
                serde_json::from_slice::<Value>(bytes)
            {
                return if within {
                    diagnostic_json_shape(telescope.response_parameters(value))
                } else {
                    "Purged By Telescope".into()
                };
            }
            if event
                .response
                .headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/plain"))
            {
                return if within {
                    text.into_owned().into()
                } else {
                    "Purged By Telescope".into()
                };
            }
        }
        if let Some(target) = &event.redirect_target {
            return format!("Redirected to {target}").into();
        }
        if let Some((path, data)) = event.rendered_view() {
            return telescope.response_parameters(json!({
                "view": path.to_string_lossy(),
                "data": diagnostic_json_shape(Value::Object(data)),
            }));
        }
        match content {
            None => "Not Captured".into(),
            Some(b"" | b"0") => "Empty Response".into(),
            Some(_) => "HTML Response".into(),
        }
    }
    fn ignore_request(&self, event: &RequestHandled) -> bool {
        let method = event.request.method.as_str();
        let method_matches = |value: &Value| {
            value
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(method))
        };
        if self
            .option("RequestWatcher", "ignore_http_methods")
            .is_some_and(|value| match value {
                Value::Array(values) => values.iter().any(method_matches),
                Value::Object(values) => values.values().any(method_matches),
                value => method_matches(&value),
            })
        {
            return true;
        }
        let status = event.response.status.as_str();
        let status_matches = |value: &Value| php_loose_string_value_equal(status, value);
        self.option("RequestWatcher", "ignore_status_codes")
            .is_some_and(|value| match value {
                Value::Array(codes) => codes.iter().any(status_matches),
                Value::Object(codes) => codes.values().any(status_matches),
                _ => false,
            })
    }
    fn record(
        &self,
        event: &dyn Event,
        telescope: &Telescope,
        context: Option<&EventContext<'_>>,
    ) -> crate::Result<()> {
        if telescope.protect_sensitive_request().is_some()
            && event.downcast_ref::<RequestHandled>().is_none()
        {
            return Ok(());
        }
        if let Some(event) =
            event.downcast_ref::<openrust::foundation::http::events::RequestReceived>()
        {
            if !self.approved_path(
                event.request.uri.path(),
                event
                    .request
                    .headers
                    .get("host")
                    .and_then(|value| value.to_str().ok()),
            ) {
                telescope.flush_entries();
                telescope.stop_recording();
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::console::events::CommandStarting>()
        {
            if matches(
                &json!([
                    // 初始化命令须先准备数据库与包资源，不能在执行前启动诊断存储。
                    "init",
                    "init:resources",
                    "migrate",
                    "migrate:rollback",
                    "migrate:fresh",
                    "migrate:reset",
                    "migrate:install",
                    "package:discover",
                    "queue:listen",
                    "queue:work",
                    "horizon",
                    "horizon:work",
                    "horizon:supervisor",
                    "telescope:list",
                    "telescope:show"
                ]),
                &event.command,
            ) || ["telescope.ignoreCommands", "telescope.ignore_commands"]
                .into_iter()
                .any(|key| {
                    self.config
                        .get(key)
                        .is_some_and(|patterns| matches(&patterns, &event.command))
                })
            {
                telescope.flush_entries();
                telescope.stop_recording();
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::console::events::CommandFinished>()
        {
            if !self.enabled("CommandWatcher")
                || self.patterns("CommandWatcher", "ignore", &event.command)
                || ["schedule:run", "schedule:finish", "package:discover"]
                    .contains(&event.command.as_str())
            {
                return Ok(());
            }
            let mut arguments = Map::new();
            let mut options = Map::new();
            arguments.insert("command".into(), event.command.clone().into());
            for argument in event.definition.get_arguments() {
                let key = argument.get_id().as_str();
                let value = event
                    .input
                    .try_get_raw(key)
                    .ok()
                    .flatten()
                    .map(|values| {
                        values
                            .map(|value| Value::from(value.to_string_lossy().into_owned()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let value = if argument.get_action().takes_values() {
                    if argument
                        .get_num_args()
                        .is_some_and(|range| range.max_values() > 1)
                    {
                        value.into()
                    } else {
                        value.into_iter().next().unwrap_or(Value::Null)
                    }
                } else {
                    event
                        .input
                        .try_get_one::<bool>(key)
                        .ok()
                        .flatten()
                        .copied()
                        .unwrap_or(false)
                        .into()
                };
                if argument.is_positional() {
                    arguments.insert(key.into(), value);
                } else {
                    options.insert(key.into(), value);
                }
            }
            telescope.record_command(IncomingEntry::make(json!({"command":event.command,"exit_code":event.exit_code,"arguments":arguments,"options":options}).as_object().unwrap().clone())?);
        } else if let Some(event) = event.downcast_ref::<RequestHandled>() {
            if !self.approved_path(
                event.request.uri.path(),
                event
                    .request
                    .headers
                    .get("host")
                    .and_then(|value| value.to_str().ok()),
            ) {
                telescope.flush_entries();
                telescope.stop_recording();
                return Ok(());
            }
            if !self.enabled("RequestWatcher") || self.ignore_request(event) {
                return Ok(());
            }
            let route = event
                .request
                .extensions
                .get::<openrust::routing::RouteMetadata>();
            let response = self.response_content(telescope, event);
            let session = event
                .session
                .clone()
                .map(|session| telescope.request_parameters(session))
                .unwrap_or_else(|| json!([]));
            let content = json!({
                "ip_address": event.ip_address,
                "method": event.request.method.as_str(),
                "uri": event.request.uri.path_and_query().map_or("/", |uri| uri.as_str()),
                "headers": telescope.headers(&event.request.headers),
                "response_status": event.response.status.as_u16(),
                "response_headers": telescope.headers(&event.response.headers),
                "duration": event.duration.as_millis(),
                "controller_action": route.map(|route| &route.action),
                "route_name": event.route_name,
                "payload": event.payload.clone().map(|payload| telescope.request_parameters(payload)),
                "session": session,
                "middleware": route.map(|route| &route.middleware),
                "response": response,
            });
            let entry = IncomingEntry::make(content.as_object().unwrap().clone())?;
            telescope.record_request(entry);
        } else if let Some(event) =
            event.downcast_ref::<openrust::database::events::QueryExecuted>()
        {
            if !self.enabled("QueryWatcher") {
                return Ok(());
            }
            let mut candidates = event
                .callers
                .iter()
                .copied()
                .filter(|caller| !Self::framework_caller(caller))
                .peekable();
            let has_candidate = candidates.peek().is_some();
            let caller = candidates.find(|caller| self.caller_visible("QueryWatcher", caller));
            if has_candidate && caller.is_none() {
                return Ok(());
            }
            let time = event.time.as_secs_f64() * 1000.0;
            let slow = self
                .option("QueryWatcher", "slow")
                .and_then(|v| v.as_f64())
                .is_some_and(|limit| time >= limit);
            let entry = IncomingEntry::make(json!({"connection":event.connection.get_name(),"driver":event.connection.get_driver_name(),"bindings":[],"sql":event.to_raw_sql()?,"time":format!("{time:.2}"),"slow":slow,"file":caller.map(std::panic::Location::file),"line":caller.map(std::panic::Location::line),"hash":format!("{:x}",md5::compute(&event.sql))}).as_object().unwrap().clone())?;
            telescope.record_query(if slow { entry.tags(["slow"]) } else { entry });
        } else if let Some(event) = event.downcast_ref::<openrust::log::events::MessageLogged>() {
            if let Some(exception) = event.exception.as_ref().and_then(Value::as_object)
                && exception.get("class").and_then(Value::as_str).is_some()
                && exception.get("message").and_then(Value::as_str).is_some()
            {
                if self.enabled("ExceptionWatcher") {
                    let mut content = exception.clone();
                    let mut context = event.context.clone();
                    context.remove("exception");
                    let tags = context
                        .remove("telescope")
                        .and_then(|value| {
                            value.as_array().map(|values| {
                                values
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect::<Vec<_>>()
                            })
                        })
                        .unwrap_or_default();
                    content.insert(
                        "context".into(),
                        if context.is_empty() {
                            Value::Null
                        } else {
                            context.into()
                        },
                    );
                    content.entry("trace").or_insert(json!([]));
                    content.entry("line_preview").or_insert(json!([]));
                    let mut exception = crate::IncomingExceptionEntry::from_report(content)?;
                    exception.0 = exception.0.tags(tags);
                    telescope.record_exception(exception);
                }
                return Ok(());
            }
            if !self.enabled("LogWatcher") {
                return Ok(());
            }
            let level = log_level(event.level);
            let minimum = self
                .option("LogWatcher", "level")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or("debug".into());
            if priority(level) < priority(&minimum) {
                return Ok(());
            }
            let mut context = event.context.clone();
            let tags = context
                .remove("telescope")
                .and_then(|v| {
                    v.as_array().map(|tags| {
                        tags.iter()
                            .filter_map(|tag| tag.as_str().map(str::to_owned))
                            .collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();
            let message = interpolate(&event.message, &context);
            telescope.record_log(
                IncomingEntry::make(
                    json!({"level":level,"message":message,"context":context})
                        .as_object()
                        .unwrap()
                        .clone(),
                )?
                .tags(tags),
            );
        } else if let Some(event) = event.downcast_ref::<openrust::cache::events::CacheHit>() {
            if self.enabled("CacheWatcher") && !self.cache_ignored(&event.key) {
                telescope.record_cache(IncomingEntry::make(json!({"type":"hit","key":event.key,"value":self.cache_value(&event.key,&event.value)}).as_object().unwrap().clone())?);
            }
        } else if let Some(event) = event.downcast_ref::<openrust::cache::events::CacheMissed>() {
            if self.enabled("CacheWatcher") && !self.cache_ignored(&event.key) {
                telescope.record_cache(IncomingEntry::make(
                    json!({"type":"missed","key":event.key})
                        .as_object()
                        .unwrap()
                        .clone(),
                )?);
            }
        } else if let Some(event) = event.downcast_ref::<openrust::cache::events::KeyWritten>() {
            if self.enabled("CacheWatcher") && !self.cache_ignored(&event.key) {
                telescope.record_cache(IncomingEntry::make(json!({"type":"set","key":event.key,"value":self.cache_value(&event.key,&event.value),"expiration":event.seconds}).as_object().unwrap().clone())?);
            }
        } else if let Some(event) = event.downcast_ref::<openrust::cache::events::KeyForgotten>() {
            if self.enabled("CacheWatcher") && !self.cache_ignored(&event.key) {
                telescope.record_cache(IncomingEntry::make(
                    json!({"type":"forget","key":event.key})
                        .as_object()
                        .unwrap()
                        .clone(),
                )?);
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::http::client::events::ResponseReceived>()
        {
            if !self.enabled("ClientRequestWatcher")
                || self
                    .option("ClientRequestWatcher", "ignore_hosts")
                    .and_then(|v| {
                        v.as_array().map(|hosts| {
                            hosts
                                .iter()
                                .any(|host| host.as_str() == event.request.url.host_str())
                        })
                    })
                    .unwrap_or(false)
            {
                return Ok(());
            }
            let mut content = client_content(telescope, &event.request);
            content.insert(
                "response_status".into(),
                event.response.status().as_u16().into(),
            );
            content.insert(
                "response_headers".into(),
                telescope.headers(event.response.headers()),
            );
            let limit = self.option("ClientRequestWatcher", "size_limit");
            let body = match event.response.resource() {
                Ok(mut resource) => {
                    let body = event.response.body().map_err(crate::Error::Http)?;
                    std::io::Seek::rewind(&mut resource).map_err(|error| {
                        crate::Error::Http(openrust::http::client::Error::ResponseBody(error))
                    })?;
                    Some(body)
                }
                Err(_) => None,
            };
            let bytes = body.as_deref().unwrap_or_default();
            let within_limit = client_response_within_limit(bytes, limit.as_ref());
            let response = {
                match body
                    .as_ref()
                    .map(|bytes| serde_json::from_slice::<Value>(bytes))
                {
                    None => "Stream Response".into(),
                    Some(Ok(value @ (Value::Array(_) | Value::Object(_)))) => {
                        if within_limit {
                            diagnostic_json_shape(telescope.response_parameters(value))
                        } else {
                            "Purged By Telescope".into()
                        }
                    }
                    _ if event
                        .response
                        .header("content-type")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| v.to_ascii_lowercase().starts_with("text/plain")) =>
                    {
                        if within_limit {
                            String::from_utf8_lossy(bytes).into_owned().into()
                        } else {
                            "Purged By Telescope".into()
                        }
                    }
                    _ if event.response.redirect() => format!(
                        "Redirected to {}",
                        event
                            .response
                            .header("location")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                    )
                    .into(),
                    _ if bytes.is_empty() || bytes == b"0" => "Empty Response".into(),
                    _ => "HTML Response".into(),
                }
            };
            content.insert("response".into(), response);
            content.insert(
                "duration".into(),
                event
                    .response
                    .transfer_stats
                    .as_ref()
                    .map(|stats| stats.get_transfer_time())
                    .filter(|seconds| *seconds > 0.0)
                    .map_or(Value::Null, |seconds| ((seconds * 1000.0) as u64).into()),
            );
            telescope.record_client_request(
                IncomingEntry::make(content)?.tags(event.request.url.host_str()),
            );
        } else if let Some(event) =
            event.downcast_ref::<openrust::http::client::events::ConnectionFailed>()
            && self.enabled("ClientRequestWatcher")
        {
            telescope.record_client_request(
                IncomingEntry::make(client_content(telescope, &event.request))?
                    .tags(event.request.url.host_str()),
            );
        }
        self.record_additional(event, telescope, context)?;
        Ok(())
    }
}
impl Listener for NativeWatcher {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if Telescope::is_storing() {
                return Ok(None);
            }
            // 启动期事件没有可用于写入诊断数据的执行作用域。
            let Ok(telescope) = facades::Telescope::get_facade_root() else {
                return Ok(None);
            };
            // Artisan 启动事件可能已在同一容器作用域内解析 Telescope。
            // 命令开始后才进入延迟回调生命周期，此时补登记本次写入。
            if !telescope.ensure_automatic_recording().map_err(|source| {
                openrust::events::EventError::Listener {
                    source: Box::new(source),
                }
            })? {
                return Ok(None);
            }
            let starting = event
                .downcast_ref::<openrust::foundation::http::events::RequestReceived>()
                .is_some()
                || event
                    .downcast_ref::<openrust::console::events::CommandStarting>()
                    .is_some();
            if starting {
                self.record(event, &telescope, Some(context))
                    .map_err(|source| openrust::events::EventError::Listener {
                        source: Box::new(source),
                    })?;
            }
            telescope.initialize().await;
            if event.downcast_ref::<openrust::support::Dumped>().is_some()
                && !self
                    .option("DumpWatcher", "always")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
                && !matches!(
                    openrust::cache::Cache::get("telescope:dump-watcher").await,
                    Ok(Some(openrust::cache::Value::Json(Value::Bool(true))))
                )
            {
                return Ok(None);
            }
            if !starting && telescope.is_recording() {
                self.record(event, &telescope, Some(context))
                    .map_err(|source| openrust::events::EventError::Listener {
                        source: Box::new(source),
                    })?;
            }
            Ok(None)
        })
    }
}
fn matches(patterns: &Value, value: &str) -> bool {
    let patterns = match patterns {
        Value::String(pattern) => vec![pattern.as_str()],
        Value::Array(patterns) => patterns.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    openrust::support::Str::is(&patterns, value, false)
}

fn log_level(level: openrust::log::Level) -> &'static str {
    use openrust::log::Level::*;
    match level {
        Debug => "debug",
        Info => "info",
        Notice => "notice",
        Warning => "warning",
        Error => "error",
        Critical => "critical",
        Alert => "alert",
        Emergency => "emergency",
    }
}
fn priority(level: &str) -> u16 {
    match level {
        "info" => 200,
        "notice" => 250,
        "warning" => 300,
        "error" => 400,
        "critical" => 500,
        "alert" => 550,
        "emergency" => 600,
        _ => 100,
    }
}
#[allow(
    clippy::cast_precision_loss,
    reason = "沿用 PHP 用浮点千字符与配置上限比较的语义"
)]
fn client_response_within_limit(bytes: &[u8], limit: Option<&Value>) -> bool {
    let characters = String::from_utf8_lossy(bytes).chars().count();
    let limit = limit
        .and_then(|value| match value {
            Value::String(text) => openrust::support::php_numeric_json_string(text, false)
                .and_then(|number| number.parse::<f64>().ok()),
            _ => value.as_f64(),
        })
        .unwrap_or(64.0);
    characters as f64 / 1000.0 <= limit
}

fn diagnostic_json_shape(value: Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(diagnostic_json_shape).collect())
        }
        Value::Object(values) => {
            if values
                .keys()
                .enumerate()
                .all(|(index, key)| key == &index.to_string())
            {
                Value::Array(values.into_values().map(diagnostic_json_shape).collect())
            } else {
                Value::Object(
                    values
                        .into_iter()
                        .map(|(key, value)| (key, diagnostic_json_shape(value)))
                        .collect(),
                )
            }
        }
        other => other,
    }
}

fn client_content(
    telescope: &Telescope,
    request: &openrust::http::client::RecordedRequest,
) -> Map<String, Value> {
    use openrust::http::client::{RecordedMultipartField, RecordedRequestDataSource};

    let payload = request.multipart.as_ref().map_or_else(
        || {
            let payload = request.diagnostic_data();
            match request.diagnostic_source {
                Some(RecordedRequestDataSource::MultipartDescriptors(_)) => {
                    telescope.multipart_descriptors(payload)
                }
                None => {
                    // 从线上正文解码的诊断载荷使用 PHP 关联数组的形状。
                    diagnostic_json_shape(telescope.request_parameters(payload))
                }
                _ => telescope.request_parameters(payload),
            }
        },
        |fields| {
            if fields.is_empty() {
                return json!([]);
            }
            let entry = |(key, field): (&String, &RecordedMultipartField)| {
                let value = match field {
                    RecordedMultipartField::Value(value) => value.clone(),
                    RecordedMultipartField::Attachment {
                        filename,
                        length,
                        headers,
                    } => {
                        let size = length.map(|bytes| {
                            let whole = bytes / 1000;
                            let fraction = bytes % 1000;
                            if fraction == 0 {
                                format!("{whole}KB")
                            } else {
                                let fraction = format!("{fraction:03}");
                                format!("{whole}.{}KB", fraction.trim_end_matches('0'))
                            }
                        });
                        let headers = if headers.is_empty() {
                            json!([])
                        } else {
                            telescope.headers(headers)
                        };
                        json!({"name":filename,"size":size,"headers":headers})
                    }
                };
                (key.clone(), value)
            };
            if let Some(RecordedRequestDataSource::Multipart(order)) = &request.diagnostic_source {
                let values: Map<String, Value> = order
                    .iter()
                    .filter_map(|key| fields.get_key_value(key))
                    .map(entry)
                    .collect();
                if values.len() == fields.len() {
                    let sequential = values
                        .keys()
                        .enumerate()
                        .all(|(index, key)| key == &index.to_string());
                    let payload = telescope.request_parameters(Value::Object(values));
                    return match (sequential, payload) {
                        (true, Value::Object(values)) => {
                            Value::Array(values.into_values().collect())
                        }
                        (_, payload) => payload,
                    };
                }
            }
            telescope.request_parameters(Value::Object(fields.iter().map(entry).collect()))
        },
    );
    json!({"method":request.method.as_str(),"uri":request.url.as_str(),"headers":telescope.headers(&request.headers),"payload":payload}).as_object().unwrap().clone()
}

fn interpolate(message: &str, context: &Map<String, Value>) -> String {
    // Scan the original string once: inserted values must not interpolate again (PHP strtr).
    let mut result = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let token = &rest[start..];
        let Some(end) = token.find('}') else {
            result.push_str(token);
            return result;
        };
        let replacement = context.get(&token[1..end]).and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(if *value { "1".into() } else { String::new() }),
            _ => None,
        });
        result.push_str(replacement.as_deref().unwrap_or(&token[..=end]));
        rest = &token[end + 1..];
    }
    result.push_str(rest);
    result
}
