use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use http::Response as NativeResponse;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
        http::Kernel,
    },
    http::{
        Body, HeaderMap, HttpError, HttpFuture, Method, Request, RequestField, RequestValue,
        Response, ResponseMacro, StatusCode, StreamedEvent, StreamedJson, UploadedFileValue,
    },
    routing::{
        ControllerDispatcher, ResourceAction, ResourceController, Router, RoutingError,
    },
    support::{
        BackedEnum, ProviderResult, ServiceProvider,
        facades::{Response as ResponseFacade, Route},
    },
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct EchoController(&'static str);

macro_rules! resource_methods {
    ($($method:ident),+) => {$ (
        fn $method<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
            Box::pin(async move {
                Response::json(StatusCode::OK, json!([self.0, stringify!($method), request.route("photo"), request.route("album"), request.route("image")]))
            })
        }
    )+};
}

impl EchoController {
    resource_methods!(index, create, store, edit, update, destroy, member);
    fn show<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let all = request.input(None).await?;
            let nested = request.input(Some("team.name")).await?;
            let exact = request.query(Some("team.name"))?;
            let upload = request.file("attachments.0").await?;
            let has_file = request.has_file("attachments").await?;
            let raw_size = request.body_bytes().await?.len();
            let response = Response::json(
                StatusCode::OK,
                json!({
                    "app": self.0, "input": all, "nested": nested, "query_exact": exact,
                    "file": upload.map(|file| json!({
                        "name": file.get_client_original_name(),
                        "path": file.get_client_original_path(),
                        "mime": file.get_client_mime_type(),
                        "bytes": file.get().as_ref(),
                        "size": file.get_size(),
                    })),
                    "has_file": has_file, "raw_size": raw_size,
                }),
            )?;
            Ok(response.header(
                ::http::header::CACHE_CONTROL,
                ::http::HeaderValue::from_static("no-store"),
            ))
        })
    }

    fn native<'a>(
        &'a self,
        _: &'a mut Request,
    ) -> HttpFuture<'a, Result<NativeResponse<Body>, HttpError>> {
        Box::pin(async { Ok(NativeResponse::new(Body::from(self.0))) })
    }

    fn direct_json<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Value, HttpError>> {
        Box::pin(async { Ok(json!({"controller": self.0})) })
    }

    fn contextual<'a>(
        &'a self,
        _: &'a mut Request,
    ) -> HttpFuture<'a, Result<ContextResponse, HttpError>> {
        Box::pin(async { Ok(ContextResponse(self.0)) })
    }
}

struct ContextResponse(&'static str);

impl openrust::contracts::support::Responsable for ContextResponse {
    async fn to_response(self, request: &mut Request) -> Result<Response, HttpError> {
        let slug = request.route("slug").unwrap_or_default().to_owned();
        if slug == "denied" {
            return Err(HttpError::NotFound);
        }
        Response::json(
            StatusCode::OK,
            json!({"label": self.0, "slug": slug, "input": request.input(Some("name")).await?}),
        )
    }
}

fn contextual_output(_: &mut Request) -> HttpFuture<'_, ContextResponse> {
    Box::pin(async { ContextResponse("borrowed") })
}

fn direct_text(request: &mut Request) -> HttpFuture<'_, Result<String, HttpError>> {
    Box::pin(async move { Ok(format!("<p>{}</p>", request.route("name").unwrap())) })
}

struct Unserializable;

impl serde::Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("unserializable route output"))
    }
}

fn native_output(request: &mut Request) -> HttpFuture<'_, Result<NativeResponse<Body>, HttpError>> {
    Box::pin(async move {
        match request.route("status") {
            Some("missing") => Err(HttpError::NotFound),
            Some(status) => Ok(NativeResponse::builder()
                .status(status.parse::<u16>().unwrap())
                .header("content-type", "text/plain")
                .header("content-length", "6")
                .header("x-native", "one")
                .header("x-native", "two")
                .body(Body::from("native"))
                .unwrap()),
            None => unreachable!("fixture requires a status"),
        }
    })
}

fn photo_resource() -> ResourceController<EchoController> {
    use ResourceAction::*;
    ResourceController::new()
        .action(Index, EchoController::index)
        .action(Create, EchoController::create)
        .action(Store, EchoController::store)
        .action(Show, EchoController::member)
        .action(Edit, EchoController::edit)
        .action(Update, EchoController::update)
        .action(Destroy, EchoController::destroy)
}

fn upload_names(value: UploadedFileValue) -> Value {
    match value {
        UploadedFileValue::File(file) => json!(file.get_client_original_name()),
        UploadedFileValue::Files(files) => Value::Object(
            files
                .into_iter()
                .map(|(key, file)| (key, upload_names(file)))
                .collect(),
        ),
        UploadedFileValue::Null => Value::Null,
    }
}

fn uploads(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({
                "files": upload_names(request.all_files().await?),
                "matched": request.file_value("photos.*.image").await?.map(upload_names),
                "empty": request.file_value("empty").await?.is_none(),
                "skipped": request.file_value("list.0").await?.is_none(),
                "single": request.file("list.1").await?.map(|file| file.get().to_vec()),
                "has_list": request.has_file("list").await?,
                "has_nested": request.has_file("photos").await?,
                "has_direct": request.has_file("photos.0").await?,
                "has_wildcard": request.has_file("photos.*.image").await?,
            }),
        )
    })
}

fn visible_input(input: &RequestValue) -> Value {
    match input {
        RequestValue::Value(value) => value.clone(),
        RequestValue::File(file) => json!({"file": file.get_client_original_name()}),
        RequestValue::Array(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.to_string(), visible_input(value)))
                .collect(),
        ),
    }
}

fn all_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let all = request.all().await?;
        let selected = request
            .all_only(&["same", "items.1.photo", "nested.name", "missing"])
            .await?;
        let required = request
            .only(&["items.*.photo", "document", "null_input", "missing"])
            .await?;
        let remainder = request
            .except(&["items.0", "document", "nested.name"])
            .await?;
        let when_has = request
            .when_has("document", async |_, value| {
                value.as_ref().map(visible_input)
            })
            .await?
            .flatten();
        let when_filled = request
            .when_filled("items.1", async |_, value| {
                value.as_ref().map(visible_input)
            })
            .await?
            .flatten();
        Response::json(
            StatusCode::OK,
            json!({
                "all": visible_input(&all),
                "keys": request.keys().await?,
                "selected": visible_input(&selected),
                "only": visible_input(&required),
                "except": visible_input(&remainder),
                "when_has": when_has,
                "when_filled": when_filled,
                "same_upload": request.file("same").await?.map(|file| file.get_client_original_name().to_owned()),
                "same_input": request.input(Some("same")).await?,
                "file_only": request.input(Some("document")).await?,
            }),
        )
    })
}

fn body_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if request.query(Some("scalar"))?.is_some() {
            request.fluent(Some("user.name")).await?;
        }
        let post = request.post(None).await?;
        let exact = request.post(Some("user.name")).await?;
        let nullable = request.post(Some("nullable")).await?;
        let post_query = request.post(Some("query")).await?;
        let mut user = request.fluent(Some("user")).await?;
        user.set("local", json!(true)).unwrap();
        let fallback = request
            .fluent_or("nullable", json!({"fallback": true}))
            .await?;
        let missing = request.fluent(Some("missing")).await?;
        let all = request.fluent(None).await?;
        let fields = request
            .fluent_only(&["user.name", "document", "missing"])
            .await?;
        let attachment = fields.get("document");
        let mut scoped = fields.scope("user");
        scoped
            .set("local", RequestValue::Value(json!(true)))
            .unwrap();
        let mut edited = fields.clone();
        if let Some(file) = attachment.clone() {
            edited.set("items.0.upload", file).unwrap();
            edited
                .set("items.*.approved", RequestValue::Value(json!(true)))
                .unwrap();
            let too_deep = format!("{}end", "path.".repeat(128));
            assert!(
                edited
                    .set(&too_deep, RequestValue::Value(Value::Null))
                    .is_err()
            );
        }
        request.merge(json!({"user.name":"updated"})).await?;
        Response::json(
            StatusCode::OK,
            json!({
                "post": post, "exact": exact, "post_null_is_absent": nullable.is_none(), "post_query": post_query,
                "fluent_user": user, "fallback": fallback, "missing_is_empty": missing.is_empty(),
                "fluent_all": all, "fluent_selected": visible_input(fields.all()),
                "attachment": attachment.as_ref().map(visible_input),
                "scoped": visible_input(scoped.all()), "edited_items": edited.get("items").as_ref().map(visible_input),
                "file_scope": visible_input(fields.scope("document").all()),
                "has_typed_user": fields.has("user.name"), "no_document_in_post": request.post(Some("document")).await?.is_none(),
                "updated": request.post(Some("user")).await?,
            }),
        )
    })
}

fn dynamic_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut fields = serde_json::Map::new();
        for key in ["slug", "model", "user.name", "missing"] {
            let value = match request.field(key).await? {
                Some(RequestField::Value(value)) => visible_input(&value),
                Some(RequestField::RouteModel(model)) => {
                    json!({"bound": model.downcast::<String>().unwrap().as_str()})
                }
                None => Value::Null,
            };
            fields.insert(key.to_owned(), value);
        }
        Response::json(StatusCode::OK, fields)
    })
}

fn native_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use axum::extract::ConnectInfo;

        let mut native = request.to_http_request().await?;
        let trace: Vec<_> = native
            .headers()
            .get_all("x-trace")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        let trace = trace.join(",");
        native.headers_mut().remove("x-trace");
        let peer = native
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .unwrap()
            .0;
        Response::json(
            StatusCode::OK,
            json!({
                "native_uri": native.uri().to_string(), "original_uri": request.original_uri().to_string(),
                "effective_host": request.host()?, "native_method": native.method().as_str(),
                "transport_method": request.get_real_method().as_str(),
                "version_matches": native.version() == request.version(),
                "trace": trace, "original_trace": request.header("x-trace")?,
                "extension_matches": request.extensions().get::<ConnectInfo<SocketAddr>>().is_some_and(|info| info.0 == peer),
                "body": std::str::from_utf8(native.body()).unwrap(), "input": request.input(Some("name")).await?,
            }),
        )
    })
}

struct Provider(&'static str);

impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let label = self.0;
            app.container()
                .bind::<EchoController, _>(move |_| Ok(Arc::new(EchoController(label))))?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        let label = self.0;
        Box::pin(async move {
            ResponseFacade::r#macro("caps", move |arguments| async move {
                let [Value::String(text)] = arguments.as_slice() else {
                    return Err(HttpError::InvalidRequest);
                };
                tokio::task::yield_now().await;
                Response::json(StatusCode::CREATED, json!([label, text.to_uppercase()]))
            })?;
            ResponseFacade::mixin(
                [
                    (
                        "caps",
                        ResponseMacro::new(|_| async { Ok(Response::no_content()) }),
                    ),
                    (
                        "replace",
                        ResponseMacro::new(|_| async { Ok(Response::no_content()) }),
                    ),
                ],
                false,
            )?;
            ResponseFacade::mixin(
                [(
                    "replace",
                    ResponseMacro::new(|_| async { Response::json(StatusCode::OK, "replaced") }),
                )],
                true,
            )?;
            Ok(())
        })
    }
}

fn response_macro(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        match request.uri().path() {
            "/macro-flush" => {
                ResponseFacade::flush_macros().map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
                Ok(Response::no_content())
            }
            "/macro-present" => {
                let present =
                    ResponseFacade::has_macro("caps").map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                Response::json(StatusCode::OK, present)
            }
            "/macro-replace" => ResponseFacade::call("replace", vec![]).await,
            _ => {
                let arguments = request.query(Some("value"))?.into_iter().collect();
                ResponseFacade::call("caps", arguments).await
            }
        }
    })
}

fn stream(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let chunks = futures_util::stream::iter([Ok::<_, std::io::Error>("first"), Ok("second")]);
        Ok(Response::stream(StatusCode::OK, chunks))
    })
}

fn stream_json(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let users = futures_util::stream::iter([Ok::<_, std::io::Error>(1), Ok(2)]);
        let empty = futures_util::stream::empty::<Result<i32, std::io::Error>>();
        let counts = futures_util::stream::iter([Ok::<_, std::io::Error>(("active", 2))]);
        Ok(Response::stream_json(
            StatusCode::OK,
            StreamedJson::object([
                ("users", StreamedJson::items(users)),
                ("counts", StreamedJson::entries(counts)),
                ("<é&", StreamedJson::value("<é>/\"&'😀\u{2028}")?),
                (
                    "meta",
                    StreamedJson::array([
                        StreamedJson::value("ready")?,
                        StreamedJson::items(empty),
                    ]),
                ),
            ]),
        ))
    })
}

fn event_stream(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if request.query(Some("fail"))?.is_some() {
            let events = futures_util::stream::iter([
                Ok(StreamedEvent::message("before failure")),
                Err(std::io::Error::other("sse fixture interruption")),
            ]);
            return Ok(Response::event_stream(events));
        }
        let events = futures_util::stream::iter([
            Ok::<_, HttpError>(StreamedEvent::message("line\r\nnext\rfinal")),
            StreamedEvent::json(
                "snapshot",
                json!({"ready":true,"text":"<é>/\"&'😀\u{2028}"}),
            ),
        ]);
        Ok(Response::event_stream(events))
    })
}

fn stream_download(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let content = futures_util::stream::iter([Ok::<_, std::io::Error>("first"), Ok("second")]);
        Response::stream_download(content, Some("résumé.txt"), HeaderMap::new(), "attachment")
    })
}

fn binary_file(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::support::facades::Config;

    Box::pin(async move {
        let root = Config::get("probe.file_root")
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?
            .expect("fixture root");
        let root = PathBuf::from(root.as_str().expect("fixture path"));
        let headers = HeaderMap::new();
        match request.uri().path() {
            "/download" => {
                Response::download(
                    root.join("document.txt"),
                    Some("résumé.txt"),
                    headers,
                    "attachment",
                )
                .await
            }
            "/download-default" => {
                Response::download(root.join("document.txt"), None, headers, "inline").await
            }
            "/file-missing" => Response::file(root.join("missing.txt"), headers).await,
            "/file-zero" => Response::file(root.join("empty.bin"), headers).await,
            _ => Response::file(root.join("document.txt"), headers).await,
        }
    })
}

fn response_factory(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use axum::http::{HeaderName, HeaderValue};
    use openrust::support::facades::View;

    Box::pin(async move {
        match request.uri().path() {
            "/response.jsonp" => {
                let callback = request.query(Some("callback"))?;
                let callback = callback
                    .as_ref()
                    .map(|value| value.as_str().ok_or(HttpError::InvalidRequest))
                    .transpose()?;
                Response::jsonp(StatusCode::OK, callback, "café / 😀 </script>")
            }
            "/response.md" => {
                let status = if request.query(Some("empty"))?.is_some() {
                    StatusCode::NOT_MODIFIED
                } else {
                    StatusCode::ACCEPTED
                };
                Ok(Response::markdown(status, "# Heading\n"))
            }
            "/response-empty" => Ok(Response::no_content().header(
                HeaderName::from_static("x-probe"),
                HeaderValue::from_static("empty"),
            )),
            _ => {
                let view = View::first(&["missing", "answer"], Default::default())
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?
                    .with("name", "<Ada>");
                let mut headers = HeaderMap::new();
                headers.insert(
                    HeaderName::from_static("x-probe"),
                    HeaderValue::from_static("view"),
                );
                Response::view(StatusCode::CREATED, view, headers, request).await
            }
        }
    })
}

fn cache_response(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use axum::http::{HeaderName, HeaderValue};

    Box::pin(async move {
        Ok(match request.uri().path() {
            "/cacheable" => {
                let debug = HeaderName::from_static("x-debug");
                let remove = HeaderName::from_static("x-remove");
                Response::new(StatusCode::OK, Body::from("cache me"))
                    .header(debug.clone(), HeaderValue::from_static("one"))
                    .header(remove.clone(), HeaderValue::from_static("two"))
                    .without_headers([debug, remove])
            }
            "/cache-date" => Response::new(StatusCode::OK, Body::from("date")),
            "/cache-manual" => Response::new(StatusCode::OK, Body::from("manual")).header(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-transform, ext=\"a,b\""),
            ),
            _ => Response::empty(StatusCode::NOT_FOUND),
        })
    })
}

fn scalar_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut values = Vec::new();
        for strict in [false, true] {
            values.push(json!({
                "integer": if strict { request.integer_strict("value", 42).await? } else { request.integer_or("value", 42).await? },
                "float": if strict { request.float_strict("value", 1.25).await? } else { request.float_or("value", 1.25).await? },
                "boolean": if strict { request.boolean_strict("value", true).await? } else { request.boolean_or("value", true).await? },
            }));
        }
        Response::json(StatusCode::OK, values)
    })
}

#[derive(serde::Serialize)]
enum Stage {
    Draft,
    Ready,
}

impl BackedEnum for Stage {
    type Backing = String;

    fn try_from_backing(backing: String) -> Option<Self> {
        match backing.as_str() {
            "draft" => Some(Self::Draft),
            "ready" => Some(Self::Ready),
            _ => None,
        }
    }
}

#[derive(serde::Serialize)]
enum Toggle {
    Off,
    On,
}

impl BackedEnum for Toggle {
    type Backing = i64;

    fn try_from_backing(backing: i64) -> Option<Self> {
        match backing {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            _ => None,
        }
    }
}

fn typed_values(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let text = request.string_strict("label", "fallback").await?.trim();
        let loose = request.str_or("label", "fallback").await?;
        let items = request.array(Some("items")).await?;
        let collected: Vec<_> = request.collect(Some("items")).await?.into_iter().collect();
        let stage = request.r#enum("stage", Some(Stage::Draft)).await?;
        let toggle = request.r#enum::<Toggle>("toggle", None).await?;
        let cases: Vec<_> = request.enums::<Stage>("list").await?.into_iter().collect();
        Response::json(
            StatusCode::OK,
            json!({"text": text, "loose": loose, "items": items, "collected": collected, "stage": stage, "toggle": toggle, "cases": cases}),
        )
    })
}

fn dated_values(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let date = request.date("when", None, None).await?;
        let formatted = request
            .date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo"))
            .await?;
        let dates = [date, formatted].map(|date| {
            date.map(|date| {
                json!([
                    date.format("Y-m-d H:i:s.u P").unwrap(),
                    date.timezone(),
                    date.microseconds(),
                    date.timestamp()
                ])
            })
        });
        let keys = ["user.name", "user.note", "missing", "query"];
        let selected = visible_input(&request.only(&keys).await?);
        let array = visible_input(&request.array_only(&keys).await?);
        let collection: Vec<_> = request
            .collect_only(&keys)
            .await?
            .into_iter()
            .map(|(key, value)| (key, visible_input(&value)))
            .collect();
        let remainder = request
            .except(&["when", "formatted", "user.secret"])
            .await?;
        let remainder = visible_input(&remainder);
        Response::json(
            StatusCode::OK,
            json!({"dates": dates, "selected": selected, "array": array, "collection": collection, "remainder": remainder}),
        )
    })
}

fn interval_values(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let interval = request.interval("duration", None).await?;
        let explicit = request.interval("seconds", Some("second")).await?;
        request.interval("invalid_unit", Some("sec")).await?;
        let values = [interval, explicit].map(|value| {
            value.map(|span| {
                json!([
                    span.years,
                    span.months,
                    span.days,
                    span.hours,
                    span.minutes,
                    span.seconds,
                    span.microseconds,
                    span.invert,
                    span.total_days()
                ])
            })
        });
        Response::json(StatusCode::OK, values)
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let (parts, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn start(label: &'static str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    start_at(label, std::env::temp_dir()).await
}

async fn start_at(label: &'static str, root: PathBuf) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    start_with(label, root, |builder| builder).await
}

async fn start_with(
    label: &'static str,
    root: PathBuf,
    configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = configure(Application::configure(root))
        .with_config(|base, _| Ok([("probe".into(), json!({"file_root": base}))].into()))
        .with_providers(vec![Box::new(Provider(label))])
        .with_routing(Routing::default().web(|| {
            Route::get("/negotiation", negotiate)?;
            Route::get("/path-encoding/{value}", path_encoding)?;
            Route::r#match(
                &[Method::GET, Method::POST],
                "/normalize-input",
                normalized_input,
            )?;
            Route::r#match(
                &[Method::GET, Method::HEAD, Method::POST, Method::PATCH],
                "/input-mutation",
                mutate_input,
            )?;
            Route::r#match(
                &[Method::GET, Method::POST],
                "/input",
                ControllerDispatcher::new(EchoController::show),
            )?;
            Route::r#match(&[Method::GET, Method::POST], "/scalars", scalar_input)?;
            Route::r#match(&[Method::POST], "/typed-values", typed_values)?;
            Route::r#match(&[Method::POST], "/dated-values", dated_values)?;
            Route::r#match(&[Method::POST], "/intervals", interval_values)?;
            Route::resource("photos", photo_resource())?;
            Route::prefix("api").group(|| {
                Route::api_resource(
                    "albums.photos",
                    photo_resource().configure(|action, route| {
                        if matches!(action, ResourceAction::Show | ResourceAction::Update) {
                            route.where_number("image")?;
                        }
                        Ok(())
                    }),
                )?
                .shallow()
                .except(&[ResourceAction::Destroy])
                .parameters(&[("photos", "image")])
                .names(&[(ResourceAction::Show, "pictures.read")])
                .scoped(&[("image", "slug")]);
                Ok(())
            })?;
            Route::singleton("profile", photo_resource())?;
            Route::api_singleton("api/albums.thumbnail", photo_resource())?
                .creatable()
                .shallow()
                .parameters(&[("albums", "photo")])
                .scoped(&[("photo", "slug")])
                .register()?;
            Route::singleton("preferences", photo_resource())?.destroyable();
            Route::get("/stream.json", stream_json)?;
            Route::get("/stream.events", event_stream)?;
            Route::get("/stream.download", stream_download)?;
            for path in ["/macro", "/macro-present", "/macro-replace"] {
                Route::get(path, response_macro)?;
            }
            Route::r#match(&[Method::POST], "/macro-flush", response_macro)?;
            for path in [
                "/file",
                "/download",
                "/download-default",
                "/file-missing",
                "/file-zero",
            ] {
                Route::get(path, binary_file)?;
            }
            Route::get("/file-cached", binary_file)?
                .name("file.cached")?
                .middleware("cache.headers:private;max_age=60;etag=release")?;
            for path in [
                "/response.jsonp",
                "/response.md",
                "/response-empty",
                "/response-view",
            ] {
                Route::get(path, response_factory)?;
            }
            Route::name("cache.")
                .middleware("cache.headers:public;max_age=2628000;etag;last_modified=1704067200")
                .group(|| {
                    Route::r#match(
                        &[Method::GET, Method::HEAD, Method::POST],
                        "/cacheable",
                        cache_response,
                    )?
                    .name("show")?;
                    Ok(())
                })?;
            Route::get_with("/cache-date", cache_response, |route| {
                route.middleware_name("cache.headers:last_modified=2024-01-01 00:00:00 UTC");
                Ok(())
            })?;
            Route::get_with("/cache-stream", stream, |route| {
                route.middleware_name("cache.headers:s_maxage=600;etag");
                Ok(())
            })?;
            Route::get_with("/cache-manual", cache_response, |route| {
                use openrust::http::middleware::SetCacheHeaders;
                route.middleware_name(SetCacheHeaders::using(
                    "s_maxage=30;private;no_transform=0;etag=manual",
                ));
                Ok(())
            })?;
            Route::get_with("/cache-invalid-status", cache_response, |route| {
                route.middleware_name("cache.headers:public;etag");
                Ok(())
            })?;
            Route::r#match(&[Method::POST], "/uploads", uploads)?;
            Route::r#match(&[Method::GET, Method::POST], "/all-input", all_input)?;
            Route::r#match(
                &[Method::GET, Method::POST, Method::PATCH],
                "/body-input",
                body_input,
            )?;
            Route::bind("model", |context| {
                Box::pin(async move {
                    Ok((context.value() != "gone").then(|| format!("model:{}", context.value())))
                })
            })?;
            Route::r#match(
                &[Method::GET, Method::POST],
                "/dynamic/{slug}/{model}",
                dynamic_input,
            )?;
            Route::r#match(&[Method::PATCH], "/native-input", native_input)?;
            Route::get("/native/sync", || NativeResponse::new(Body::from("sync")))?;
            Route::get("/native/async", || async {
                Ok::<_, HttpError>(NativeResponse::new(Body::from("async")))
            })?;
            Route::get(
                "/native/controller",
                ControllerDispatcher::new(EchoController::native),
            )?;
            Route::get("/native/{status}", native_output)?;
            Route::get("/direct/text", || "<p>你好</p>")?;
            Route::get("/direct/text/{name}", direct_text)?;
            Route::get("/direct/stringable", || {
                openrust::support::Stringable::from("<b>text</b>")
            })?;
            Route::get("/direct/array", || [1, 2, 3])?;
            Route::get("/direct/vector", || async { vec!["hello", "世界"] })?;
            Route::get("/direct/json", || json!({"value": "中/😀<&"}))?;
            Route::get("/direct/sparse", || {
                openrust::support::Collection::from_entries([(2_i64, 9)])
            })?;
            Route::get("/direct/fluent", || {
                openrust::support::Fluent::make(json!({"name":"Ada"})).unwrap()
            })?;
            Route::get(
                "/direct/controller",
                ControllerDispatcher::new(EchoController::direct_json),
            )?;
            Route::get("/direct/error", || [Unserializable])?;
            Route::get_with(
                "/responsable/{slug}",
                || ContextResponse("sync"),
                |route| {
                    route.middleware_name("cache.headers:private;max_age=60");
                    Ok(())
                },
            )?;
            Route::r#match(&[Method::POST], "/responsable/async/{slug}", || async {
                Ok::<_, HttpError>(ContextResponse("async"))
            })?;
            Route::r#match(
                &[Method::POST],
                "/responsable/borrowed/{slug}",
                contextual_output,
            )?;
            Route::get(
                "/responsable/controller/{slug}",
                ControllerDispatcher::new(EchoController::contextual),
            )?;
            Route::get("/stream", stream)?;
            Ok(())
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport
        .with_state(Arc::new(kernel))
        .into_make_service_with_connect_info::<SocketAddr>();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

fn path_encoding(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move { Response::json(StatusCode::OK, json!(request.decoded_path())) })
}

#[tokio::test]
async fn real_http_checks_path_encoding_and_both_body_size_boundaries() {
    use std::time::Duration;

    let (address, server) = start("transport-validation").await;
    for (path, expected) in [
        ("caf%C3%A9", "path-encoding/café"),
        ("literal%ZZ", "path-encoding/literal%ZZ"),
        ("%25FF", "path-encoding/%FF"),
    ] {
        let (_, body) = send(
            address,
            "GET",
            &format!("/path-encoding/{path}"),
            "",
            b"",
            200,
        )
        .await;
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), expected);
    }
    for path in [
        "/path-encoding/%FF",
        "/path-encoding/%C0%AF",
        "/missing/%ED%A0%80",
    ] {
        let (_, body) = send(address, "GET", path, "", b"", 400).await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["message"],
            "Malformed URL."
        );
    }

    async fn read_response(socket: &mut tokio::net::TcpStream) -> String {
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(5), socket.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        response
    }

    for (length, expected) in [(8 * 1024 * 1024, 204), (8 * 1024 * 1024 + 1, 413)] {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let head = format!(
            "GET /response-empty HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Length: {length}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n"
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        let response = read_response(&mut socket).await;
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected} ")),
            "{response}"
        );
        if expected == 413 {
            assert!(
                response.contains("The POST data is too large."),
                "{response}"
            );
        }
    }

    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let head = format!(
        "POST /normalize-input HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n800001\r\n"
    );
    socket.write_all(head.as_bytes()).await.unwrap();
    socket
        .write_all(&vec![b'x'; 8 * 1024 * 1024 + 1])
        .await
        .unwrap();
    let response = read_response(&mut socket).await;
    assert!(response.starts_with("HTTP/1.1 413 "), "{response}");
    server.abort();
}

fn normalized_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.input(None).await?;
        let body = request.body_bytes().await?;
        let upload = request.file("document").await?;
        let text = std::str::from_utf8(&body).map_err(|_| HttpError::InvalidRequest)?;
        Response::json(
            StatusCode::OK,
            json!({
                "input": input,
                "query": request.query(None)?,
                "raw_query": request.uri().query(),
                "raw_body": text,
                "raw_form": request.raw_form_fields().await?,
                "upload_text": upload.map(|file| String::from_utf8_lossy(file.get().as_ref()).into_owned()),
            }),
        )
    })
}

#[tokio::test]
async fn real_http_normalizes_input_bags_and_keeps_application_exceptions_separate() {
    let root = tempfile::tempdir().unwrap();
    let ((custom, one), (default, two)) = tokio::join!(
        start_with("custom", root.path().to_owned(), |builder| {
            builder.with_middleware(|middleware| {
                middleware
                    .trim_strings(["rows.*.sealed"])
                    .skip_trim_strings_when(|request| request.has_header("x-no-trim"))
                    .convert_empty_strings_to_null(|request| {
                        request.query(Some("keep")).ok().flatten() == Some(json!("yes"))
                    });
            })
        }),
        start("normalization-default"),
    );
    let raw = r#"{"name":"\u00a0 Ada \u200b","blank":"  ","password":" secret ","password_confirmation":"","rows":[{"sealed":" keep ","password":" nested "}],"zero":0,"flag":false}"#;
    let uri = "/normalize-input?from=%20query%20&empty=";
    let (_, body) = send(custom, "POST", uri, "application/json", raw.as_bytes(), 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value["input"],
        json!({
            "name":"Ada", "blank":null, "password":" secret ", "password_confirmation":null,
            "rows":[{"sealed":" keep ","password":"nested"}], "zero":0, "flag":false,
            "from":"query", "empty":null,
        })
    );
    assert_eq!(value["raw_body"], raw);
    assert_eq!(value["raw_query"], "from=%20query%20&empty=");
    assert_eq!(value["query"], json!({"from":"query","empty":null}));

    let (_, body) = send(
        default,
        "POST",
        uri,
        "application/json",
        raw.as_bytes(),
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["input"]["rows"][0]["sealed"], "keep");

    let (_, body) = send(
        custom,
        "GET",
        "/normalize-input?keep=%20yes%20&blank=%20%20",
        "text/plain",
        b"",
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["input"], json!({"keep":"yes", "blank":""}));

    let (_, body) = send_with_headers(
        custom,
        "POST",
        "/normalize-input",
        "application/x-www-form-urlencoded",
        b"name=+keep+&spaces=++&empty=",
        200,
        "X-No-Trim: yes\r\n",
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value["input"],
        json!({"name":" keep ", "spaces":"  ", "empty":null})
    );

    let multipart = b"--fixture\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\n  Ada  \r\n--fixture\r\nContent-Disposition: form-data; name=\"document\"; filename=\"sample.txt\"\r\n\r\n  file  \r\n--fixture--\r\n";
    let (_, body) = send(
        custom,
        "POST",
        "/normalize-input",
        "multipart/form-data; boundary=fixture",
        multipart,
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["input"]["name"], "Ada");
    assert_eq!(value["raw_form"], json!({"name":"  Ada  "}));
    assert_eq!(value["upload_text"], "  file  ");
    assert_eq!(value["raw_body"], std::str::from_utf8(multipart).unwrap());

    let expanded = json!({("k".repeat(10_000)): vec![""; 2_100]}).to_string();
    send(
        custom,
        "POST",
        "/normalize-input",
        "application/json",
        expanded.as_bytes(),
        413,
    )
    .await;
    one.abort();
    two.abort();
}

fn mutate_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let body = request.body_bytes().await?;
        let original = request.input(None).await?;
        let present = request.has(&["nullable", "fallback"]).await?;
        let any_present = request.has_any(&["absent", "fallback"]).await?;
        let exists = request.exists(&["fallback"]).await?;
        let upload = request
            .when_has("document", async |_, value| match value {
                Some(RequestValue::File(file)) => Some(file),
                _ => None,
            })
            .await?
            .flatten();
        let file_is_filled = request.filled(&["document"]).await?;
        let not_filled = request.is_not_filled(&["blank", "absent"]).await?;
        let falsy_is_filled = request.filled(&["zero", "flag", "empty"]).await?;
        let any_filled = request.any_filled(&["absent", "fallback"]).await?;
        let callback = request
            .when_filled("flag", async |_, value| value.as_ref().map(visible_input))
            .await?;
        let absent = request.when_missing("absent", async |_| "missing").await?;
        request
            .merge_if_missing(json!({"nullable":"changed","fallback":"changed","document":"changed","added":" padded "}))
            .await?;
        let invalid = format!("{}.end", "too.".repeat(128));
        assert!(matches!(
            request
                .merge(json!({"should_rollback":true,(invalid):true}))
                .await,
            Err(HttpError::InvalidRequest)
        ));
        assert!(request.missing(&["should_rollback"]).await?);
        request.merge(json!({"profile.age": 42})).await?;
        let merged = request.input(None).await?;
        let merged_json = if request.is_json() {
            request.json(None).await?
        } else {
            None
        };
        request
            .parts_mut()
            .headers
            .insert("x-checked", ::http::HeaderValue::from_static("yes"));
        assert_eq!(request.input(None).await?, merged);
        request.replace(json!({"only":true})).await?;
        let replaced_json = if request.is_json() {
            request.json(None).await?
        } else {
            None
        };
        Response::json(
            StatusCode::OK,
            json!({
                "original": original,
                "present": present,
                "any_present": any_present,
                "exists": exists,
                "not_filled": not_filled,
                "falsy_is_filled": falsy_is_filled,
                "any_filled": any_filled,
                "callback": callback,
                "absent": absent,
                "upload": upload.map(|file| file.get_client_original_name().to_string()),
                "file_is_filled": file_is_filled,
                "merged": merged,
                "merged_json": merged_json,
                "replaced": request.input(None).await?,
                "replaced_json": replaced_json,
                "query": request.query(None)?,
                "original_uri_query": request.uri().query(),
                "raw_preserved": request.body_bytes().await? == body,
                "upload_preserved": request.has_file("document").await?,
                "method": request.method().as_str(),
                "real_method": request.get_real_method().as_str(),
            }),
        )
    })
}

#[tokio::test]
async fn real_http_mutates_the_selected_input_source_without_rewriting_transport_data() {
    let (address, server) = start("input-mutation").await;
    let path = "/input-mutation?fallback=query&nullable=query";
    let (_, body) = send(
        address,
        "POST",
        path,
        "application/json",
        br#"{"nullable":null,"blank":" ","zero":0,"flag":false,"empty":[],"profile":{"name":"Ada"}}"#,
        200,
    )
    .await;
    let data: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(data["present"], true);
    assert_eq!(data["any_present"], true);
    assert_eq!(data["exists"], true);
    assert_eq!(data["not_filled"], true);
    assert_eq!(data["falsy_is_filled"], true);
    assert_eq!(data["any_filled"], true);
    assert_eq!(data["callback"], false);
    assert_eq!(data["absent"], "missing");
    assert_eq!(data["merged"]["nullable"], Value::Null);
    assert_eq!(data["merged"]["fallback"], "query");
    assert_eq!(data["merged"]["profile"], json!({"name":"Ada","age":42}));
    assert_eq!(data["merged"]["added"], " padded ");
    assert_eq!(data["merged_json"]["profile"], data["merged"]["profile"]);
    assert!(data["merged_json"].get("fallback").is_none());
    assert_eq!(data["replaced_json"], json!({"only":true}));
    assert_eq!(
        data["replaced"],
        json!({"only":true,"fallback":"query","nullable":"query"})
    );
    assert_eq!(data["raw_preserved"], true);

    let (_, body) = send(
        address,
        "GET",
        path,
        "application/x-www-form-urlencoded",
        b"name=body",
        200,
    )
    .await;
    let data: Value = serde_json::from_str(&body).unwrap();
    assert!(data["original"].get("name").is_none());
    assert_eq!(data["replaced"], json!({"only":true}));
    assert_eq!(data["query"], data["replaced"]);
    assert_eq!(data["original_uri_query"], "fallback=query&nullable=query");
    assert_eq!(data["raw_preserved"], true);

    let (_, body) = send(
        address,
        "POST",
        path,
        "application/x-www-form-urlencoded",
        b"_method=PATCH&profile[name]=Ada",
        200,
    )
    .await;
    let data: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(data["method"], "PATCH");
    assert_eq!(data["real_method"], "POST");
    assert_eq!(data["merged"]["profile"], json!({"name":"Ada","age":42}));
    assert_eq!(
        data["replaced"],
        json!({"only":true,"fallback":"query","nullable":"query"})
    );

    let multipart = b"--fixture\r\nContent-Disposition: form-data; name=\"document\"; filename=\"report.txt\"\r\n\r\nhello\r\n--fixture--\r\n";
    let (_, body) = send(
        address,
        "POST",
        path,
        "multipart/form-data; boundary=fixture",
        multipart,
        200,
    )
    .await;
    let data: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(data["upload"], "report.txt");
    assert_eq!(data["file_is_filled"], false);
    assert!(data["merged"].get("document").is_none());
    assert_eq!(data["upload_preserved"], true);
    assert_eq!(data["raw_preserved"], true);

    send(address, "POST", path, "application/json", b"{ invalid", 400).await;
    server.abort();
}

fn negotiate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({
                "types": request.get_acceptable_content_types(),
                "preferred": request.prefers(&["html", "json", "text/markdown"]),
                "format": request.format("fallback"),
                "accepts": [request.accepts_html(), request.accepts_json(), request.accepts_markdown()],
                "wants": [request.wants_json(), request.wants_markdown()],
                "expects_json": request.expects_json(),
                "is_json": request.is_json(),
            }),
        )
    })
}

#[tokio::test]
async fn reference_content_negotiation_uses_quality_order_and_media_types_over_http() {
    let (address, server) = start("negotiation").await;
    let client = reqwest::Client::new();
    let url = format!("http://{address}/negotiation");
    let cases = [
        (
            "text/html;profile=\"one,two;q=0\";q=.4, application/problem+json;q=.9",
            json!([
                [
                    "application/problem+json",
                    "text/html;profile=\"one,two;q=0\""
                ],
                "json",
                "problem",
                [true, false, false],
                [true, false],
                true
            ]),
        ),
        (
            "application/json;q=.5,text/html;q=.5,application/json;q=.5",
            json!([
                ["text/html", "application/json"],
                "html",
                "html",
                [true, true, false],
                [false, false],
                false
            ]),
        ),
        (
            "text/markdown;charset=UTF-8,application/*;q=.5",
            json!([
                ["text/markdown;charset=UTF-8", "application/*"],
                "text/markdown",
                "fallback",
                [false, true, true],
                [false, true],
                false
            ]),
        ),
        (
            "application/json;q=0",
            json!([
                ["application/json"],
                "json",
                "json",
                [false, true, false],
                [true, false],
                true
            ]),
        ),
        (
            "application/*",
            json!([
                ["application/*"],
                "json",
                "fallback",
                [false, true, false],
                [false, false],
                false
            ]),
        ),
        (
            "*/*",
            json!([
                ["*/*"],
                "html",
                "fallback",
                [true, true, true],
                [false, false],
                true
            ]),
        ),
    ];
    for (accept, expected) in cases {
        let body: Value = client
            .get(&url)
            .header("accept", accept)
            .header("x-requested-with", "XMLHttpRequest")
            .header("content-type", "application/problem+json")
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            json!([
                body["types"],
                body["preferred"],
                body["format"],
                body["accepts"],
                body["wants"],
                body["expects_json"]
            ]),
            expected,
            "{accept}"
        );
        assert_eq!(body["is_json"], true);
    }
    let response: Value = client
        .get(&url)
        .header("accept", "*/*")
        .header("x-requested-with", "XMLHttpRequest")
        .header("x-pjax", "1")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["expects_json"], false);

    let response: Value = client
        .get(&url)
        .header("accept", "text/html")
        .header("accept", "application/json")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["types"], json!(["text/html"]));
    assert_eq!(response["wants"], json!([false, false]));
    assert!(Request::matches_type(
        "application/json",
        "application/problem+json"
    ));
    server.abort();
}

async fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    media: &str,
    body: &[u8],
    status: u16,
) -> (String, String) {
    send_with_headers(address, method, path, media, body, status, "").await
}

async fn send_with_headers(
    address: SocketAddr,
    method: &str,
    path: &str,
    media: &str,
    body: &[u8],
    status: u16,
    additional: &str,
) -> (String, String) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n{additional}\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(body).await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(
        headers.starts_with(&format!("HTTP/1.1 {status} ")),
        "{response}"
    );
    (headers.to_owned(), body.to_owned())
}

#[tokio::test]
async fn real_http_dispatches_controllers_and_reads_json_form_uploads_and_streams() {
    let ((first, one), (second, two)) = tokio::join!(start("first"), start("second"));
    let json_body = br#"{"team":{"name":"Ada"},"present":null}"#;
    let query = "/input?team[name]=query&present=query&keep=1";
    let ((headers, body), (_, second_body)) = tokio::join!(
        send(
            first,
            "POST",
            query,
            "application/vnd.example+json",
            json_body,
            200
        ),
        send(second, "POST", query, "application/json", json_body, 200),
    );
    assert!(headers.contains("\r\ncontent-type: application/json"));
    assert!(headers.contains("\r\ncache-control: no-store"));
    let first_json: Value = serde_json::from_str(&body).unwrap();
    let second_json: Value = serde_json::from_str(&second_body).unwrap();
    assert_eq!(first_json["app"], "first");
    assert_eq!(second_json["app"], "second");
    assert_eq!(
        first_json["input"],
        json!({"team":{"name":"Ada"},"present":null,"keep":"1"})
    );
    assert_eq!(first_json["nested"], "Ada");
    assert!(first_json["query_exact"].is_null());
    assert_eq!(first_json["raw_size"], json_body.len());

    let form = b"team[name]=form&tag[]=a&tag[]=b&key=old&key=new";
    let (_, body) = send(
        first,
        "POST",
        query,
        "application/x-www-form-urlencoded",
        form,
        200,
    )
    .await;
    let form_json: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(form_json["input"]["tag"], json!(["a", "b"]));
    assert_eq!(form_json["input"]["key"], "new");
    assert_eq!(form_json["nested"], "form");
    let (_, body) = send(
        first,
        "GET",
        query,
        "application/x-www-form-urlencoded",
        form,
        200,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["nested"],
        "query"
    );

    let multipart = b"--probe\r\nContent-Disposition: form-data; name=\"team[name]\"\r\n\r\nupload\r\n--probe\r\nContent-Disposition: form-data; name=\"attachments[]\"; filename=\"sub/example.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n\x00\xff\r\n--probe--\r\n";
    let (_, body) = send(
        first,
        "POST",
        query,
        "multipart/form-data; boundary=probe",
        multipart,
        200,
    )
    .await;
    let upload: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(upload["nested"], "upload");
    assert_eq!(upload["has_file"], true);
    assert_eq!(
        upload["file"],
        json!({"name":"example.bin","path":"sub/example.bin","mime":"application/octet-stream","bytes":[0,255],"size":2})
    );
    assert_eq!(upload["raw_size"], multipart.len());

    for (media, invalid) in [
        ("application/json", b"{broken".as_slice()),
        ("application/x-www-form-urlencoded", b"name=%FF".as_slice()),
        ("multipart/form-data", b"no-boundary".as_slice()),
    ] {
        let (_, body) = send(first, "POST", "/input", media, invalid, 400).await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["message"],
            "Bad Request"
        );
    }
    let mut socket = tokio::net::TcpStream::connect(first).await.unwrap();
    socket.write_all(
        format!("POST /input HTTP/1.1\r\nHost: {first}\r\nContent-Type: application/json\r\nContent-Length: 8388609\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    ).await.unwrap();
    let mut rejected = String::new();
    socket.read_to_string(&mut rejected).await.unwrap();
    assert!(rejected.starts_with("HTTP/1.1 413 "), "{rejected}");
    let (headers, body) = send(first, "GET", "/stream", "", b"", 200).await;
    assert!(headers.contains("\r\nx-accel-buffering: no"));
    assert!(headers.contains("\r\ntransfer-encoding: chunked"));
    assert_eq!(body, "5\r\nfirst\r\n6\r\nsecond\r\n0\r\n\r\n");
    one.abort();
    two.abort();
}

#[tokio::test]
async fn real_http_reads_grouped_uploads_without_reindexing_empty_slots() {
    let (address, server) = start("uploads").await;
    let body = concat!(
        "--files\r\nContent-Disposition: form-data; name=\"empty\"; filename=\"\"\r\n\r\n\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"list[]\"; filename=\"\"\r\n\r\n\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"list[]\"; filename=\"zero.txt\"\r\n\r\n\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"photos[0][image]\"; filename=\"first.txt\"\r\n\r\none\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"photos[1][image]\"; filename=\"second.txt\"\r\n\r\ntwo\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"photos[2][other]\"; filename=\"other.txt\"\r\n\r\nthree\r\n",
        "--files\r\nContent-Disposition: form-data; name=\"named[missing]\"; filename=\"\"\r\n\r\n\r\n",
        "--files--\r\n",
    );
    let (_, response) = send(
        address,
        "POST",
        "/uploads",
        "multipart/form-data; boundary=files",
        body.as_bytes(),
        200,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap(),
        json!({
            "files": {
                "list": {"1": "zero.txt"},
                "photos": {"0": {"image": "first.txt"}, "1": {"image": "second.txt"}, "2": {"other": "other.txt"}},
            },
            "matched": {"0": "first.txt", "1": "second.txt", "2": null},
            "empty": true, "skipped": true, "single": [],
            "has_list": true, "has_nested": false, "has_direct": true, "has_wildcard": true,
        }),
    );
    server.abort();
}

#[tokio::test]
async fn real_http_merges_all_input_and_files_with_input_precedence() {
    let (address, server) = start("all-input").await;
    let body = concat!(
        "--fields\r\nContent-Disposition: form-data; name=\"same\"\r\n\r\ntext\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"items[0][name]\"\r\n\r\none\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"items[1][name]\"\r\n\r\ntwo\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"null_input\"\r\n\r\n\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"same\"; filename=\"collision.txt\"\r\n\r\none\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"items[1][photo]\"; filename=\"photo.txt\"\r\n\r\ntwo\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"items[1][name]\"; filename=\"shadowed.txt\"\r\n\r\nthree\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"null_input[child]\"; filename=\"null.txt\"\r\n\r\nfour\r\n",
        "--fields\r\nContent-Disposition: form-data; name=\"document\"; filename=\"report.txt\"\r\n\r\nfive\r\n",
        "--fields--\r\n",
    );
    let (_, response) = send(
        address,
        "POST",
        "/all-input?same=query&query=kept",
        "multipart/form-data; boundary=fields",
        body.as_bytes(),
        200,
    )
    .await;
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        response,
        json!({
            "all": {
                "same": "text", "items": {"0": {"name": "one"}, "1": {"name": "two", "photo": {"file": "photo.txt"}}},
                "null_input": null, "query": "kept", "document": {"file": "report.txt"},
            },
            "keys": ["same", "items", "null_input", "query", "same", "items", "null_input", "document"],
            "selected": {"same": "text", "items": {"1": {"photo": {"file": "photo.txt"}}}, "nested": {"name": null}, "missing": null},
        "only": {"items": {"*": {"photo": {"0": null, "1": {"file": "photo.txt"}}}}, "document": {"file": "report.txt"}, "null_input": null},
        "except": {"same": "text", "items": {"1": {"name": "two", "photo": {"file": "photo.txt"}}}, "null_input": null, "query": "kept"},
        "when_has": {"file": "report.txt"},
        "when_filled": {"name": "two", "photo": {"file": "photo.txt"}},
            "same_upload": "collision.txt", "same_input": "text", "file_only": null,
        })
    );
    assert_eq!(
        response["all"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["same", "items", "null_input", "query", "document"],
    );
    assert_eq!(
        response["all"]["items"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["0", "1"],
    );

    let (_, response) = send(
        address, "POST", "/all-input?2=query", "application/json",
        br#"{"same":"json","0":"zero","01":"leading","nested":{"name":"inner"},"nested.name":"exact"}"#, 200,
    ).await;
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        response["keys"],
        json!(["same", 0, "01", "nested", "nested.name", 2])
    );
    assert_eq!(response["selected"]["nested"]["name"], "exact");
    assert_eq!(response["all"]["nested"]["name"], "inner");
    assert_eq!(response["except"]["nested"]["name"], "inner");
    assert!(response["except"].get("nested.name").is_none());
    send(
        address,
        "POST",
        "/all-input",
        "multipart/form-data",
        b"bad",
        400,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn real_http_reads_body_and_fluent_values_with_their_own_sources() {
    let (address, server) = start("body-input").await;
    let (_, response) = send(
        address,
        "PATCH",
        "/body-input?query=keep",
        "application/x-www-form-urlencoded",
        b"user[name]=+Ada+&nullable=",
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        value["post"],
        json!({"user":{"name":"Ada"},"nullable":null})
    );
    assert_eq!(value["post_null_is_absent"], true);
    assert!(value["post_query"].is_null());
    assert!(value["exact"].is_null());
    assert_eq!(value["fluent_user"], json!({"name":"Ada","local":true}));
    assert_eq!(value["fallback"], json!({"fallback":true}));
    assert_eq!(value["missing_is_empty"], true);
    assert_eq!(value["fluent_all"]["query"], "keep");
    assert_eq!(value["fluent_selected"], json!({"user":{"name":"Ada"}}));
    assert_eq!(value["updated"], json!({"name":"updated"}));

    let (_, response) = send(
        address,
        "POST",
        "/body-input?query=keep",
        "application/json",
        br#"{"user":{"name":"nested"},"user.name":"literal","nullable":null}"#,
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(value["exact"], "literal");
    assert_eq!(value["fluent_user"]["name"], "nested");
    assert!(value["post_query"].is_null());
    assert_eq!(value["updated"], json!({"name":"updated"}));
    send(
        address,
        "POST",
        "/body-input?scalar=1",
        "application/json",
        br#"{"user":{"name":"scalar"}}"#,
        400,
    )
    .await;

    let body = concat!(
        "--body\r\nContent-Disposition: form-data; name=\"user[name]\"\r\n\r\nGrace\r\n",
        "--body\r\nContent-Disposition: form-data; name=\"document\"; filename=\"report.txt\"\r\n\r\nfile\r\n",
        "--body--\r\n",
    );
    let (_, response) = send(
        address,
        "POST",
        "/body-input",
        "multipart/form-data; boundary=body",
        body.as_bytes(),
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(value["post"], json!({"user":{"name":"Grace"}}));
    assert_eq!(value["no_document_in_post"], true);
    assert_eq!(value["has_typed_user"], true);
    assert_eq!(
        value["fluent_selected"],
        json!({"user":{"name":"Grace"},"document":{"file":"report.txt"}})
    );
    assert_eq!(value["attachment"], json!({"file":"report.txt"}));
    assert_eq!(value["scoped"], json!({"name":"Grace","local":true}));
    assert_eq!(
        value["edited_items"],
        json!({"0":{"upload":{"file":"report.txt"},"approved":true}})
    );
    assert_eq!(value["file_scope"], json!({"0":{"file":"report.txt"}}));
    assert!(value["fluent_all"].get("document").is_none());
    send(
        address,
        "POST",
        "/body-input",
        "application/json",
        b"{broken",
        400,
    )
    .await;

    let (_, response) = send(address, "GET", "/body-input?user[name]=query", "", b"", 200).await;
    let value: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(value["post"], json!([]));
    assert_eq!(value["fluent_user"]["name"], "query");
    assert!(value["updated"].is_null());
    server.abort();
}

#[tokio::test]
async fn real_http_dynamic_field_lookup_falls_back_to_raw_and_bound_route_values() {
    let (address, server) = start("dynamic-fields").await;
    let (_, body) = send(address, "GET", "/dynamic/route/7", "", b"", 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"slug":"route","model":{"bound":"model:7"},"user.name":null,"missing":null})
    );
    let (_, body) = send(
        address,
        "POST",
        "/dynamic/route/7?slug=query",
        "application/json",
        br#"{"slug":null,"model":"input","user":{"name":"nested"},"user.name":"literal"}"#,
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"slug":null,"model":"input","user.name":"literal","missing":null})
    );
    let multipart = b"--dynamic\r\nContent-Disposition: form-data; name=\"slug\"; filename=\"upload.txt\"\r\n\r\nbytes\r\n--dynamic--\r\n";
    let (_, body) = send(
        address,
        "POST",
        "/dynamic/route/7",
        "multipart/form-data; boundary=dynamic",
        multipart,
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["slug"], json!({"file":"upload.txt"}));
    assert_eq!(value["model"], json!({"bound":"model:7"}));
    send(address, "GET", "/dynamic/route/gone", "", b"", 404).await;
    server.abort();
}

#[tokio::test]
async fn native_request_conversion_preserves_a_lazy_body_and_transport_parts() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    let body = Body::from_stream(futures_util::stream::once(async move {
        observed.fetch_add(1, Ordering::Relaxed);
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"body"))
    }));
    let mut native = ::http::Request::builder()
        .version(::http::Version::HTTP_2)
        .method("POST")
        .uri("/raw%2Ftarget?key=%2B")
        .extension(42_u32)
        .body(body)
        .unwrap();
    native
        .headers_mut()
        .append("x-trace", ::http::HeaderValue::from_static("first"));
    native
        .headers_mut()
        .append("x-trace", ::http::HeaderValue::from_static("second"));
    let framework = Request::from(native);
    assert_eq!(framework.version(), ::http::Version::HTTP_2);
    assert_eq!(framework.original_uri(), "/raw%2Ftarget?key=%2B");
    assert_eq!(framework.extensions().get::<u32>(), Some(&42));
    let native: ::http::Request<Body> = framework.into();
    assert_eq!(polls.load(Ordering::Relaxed), 0);
    assert_eq!(native.method(), "POST");
    assert_eq!(native.headers().get_all("x-trace").iter().count(), 2);
    assert_eq!(
        axum::body::to_bytes(native.into_body(), 32).await.unwrap(),
        "body"
    );
    assert_eq!(polls.load(Ordering::Relaxed), 1);

    let native = ::http::Request::builder()
        .header("content-length", "8388609")
        .body(Body::empty())
        .unwrap();
    assert!(matches!(
        Request::from(native).to_http_request().await,
        Err(HttpError::PayloadTooLarge)
    ));
}

#[tokio::test]
async fn real_http_native_snapshot_keeps_the_transport_target_and_effective_method() {
    use openrust::http::TrustedProxyHeaders;

    let (address, server) = start_with("native", std::env::temp_dir(), |builder| {
        builder.with_middleware(|middleware| {
            middleware
                .trust_proxies(&["127.0.0.1"], TrustedProxyHeaders::DEFAULT)
                .unwrap();
        })
    })
    .await;
    let (_, body) = send_with_headers(
        address, "POST", "/native-input?token=%2B", "application/x-www-form-urlencoded",
        b"_method=PATCH&name=+Ada+", 200,
        "X-Trace: one\r\nX-Trace: two\r\nX-Forwarded-Host: public.example.test\r\nX-Forwarded-Proto: https\r\n",
    ).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({
            "native_uri":"/native-input?token=%2B", "original_uri":"/native-input?token=%2B",
            "effective_host":"public.example.test", "native_method":"PATCH", "transport_method":"POST",
            "version_matches":true, "trace":"one,two", "original_trace":"one", "extension_matches":true,
            "body":"_method=PATCH&name=+Ada+", "input":"Ada",
        })
    );
    server.abort();
}

#[tokio::test]
async fn native_response_round_trip_preserves_parts_and_does_not_poll_the_stream() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    let stream = futures_util::stream::once(async move {
        observed.fetch_add(1, Ordering::Relaxed);
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"native"))
    });
    let native = NativeResponse::builder()
        .status(202)
        .version(::http::Version::HTTP_2)
        .extension(17_u32)
        .header("x-native", "one")
        .header("x-native", "two")
        .body(Body::from_stream(stream))
        .unwrap();
    let response: NativeResponse<Body> = Response::from(native).into();
    assert_eq!(polls.load(Ordering::Relaxed), 0);
    assert_eq!(response.status(), 202);
    assert_eq!(response.version(), ::http::Version::HTTP_2);
    assert_eq!(response.extensions().get::<u32>(), Some(&17));
    assert_eq!(response.headers().get_all("x-native").iter().count(), 2);
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 32)
            .await
            .unwrap(),
        "native"
    );
    assert_eq!(polls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn real_http_native_route_responses_run_through_response_preparation() {
    let (address, server) = start("native-controller").await;
    for (path, expected) in [
        ("/native/sync", "sync"),
        ("/native/async", "async"),
        ("/native/controller", "native-controller"),
    ] {
        let (_, body) = send(address, "GET", path, "", b"", 200).await;
        assert_eq!(body, expected);
    }
    let (headers, body) = send(address, "GET", "/native/202", "", b"", 202).await;
    assert_eq!(body, "native");
    assert!(headers.contains("\r\nx-native: one\r\nx-native: two"));
    let (headers, body) = send(address, "HEAD", "/native/202", "", b"", 202).await;
    assert!(body.is_empty() && headers.contains("\r\ncontent-length: 6"));
    for status in [204, 304] {
        let (headers, body) = send(
            address,
            "GET",
            &format!("/native/{status}"),
            "",
            b"",
            status,
        )
        .await;
        assert!(body.is_empty());
        assert!(!headers.contains("content-length") && !headers.contains("content-type"));
    }
    let (_, body) = send(address, "GET", "/native/missing", "", b"", 404).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["message"],
        "Not Found"
    );
    server.abort();
}

#[tokio::test]
async fn real_http_renders_direct_html_and_structured_return_values() {
    let (address, server) = start("structured").await;
    for (path, expected) in [
        ("/direct/text", "<p>你好</p>"),
        ("/direct/text/Ada", "<p>Ada</p>"),
        ("/direct/stringable", "<b>text</b>"),
    ] {
        let (headers, body) = send(address, "GET", path, "", b"", 200).await;
        assert_eq!(body, expected);
        assert!(headers.contains("\r\ncontent-type: text/html; charset=utf-8"));
    }
    for (path, expected) in [
        ("/direct/array", "[1,2,3]"),
        ("/direct/vector", r#"["hello","\u4e16\u754c"]"#),
        ("/direct/json", r#"{"value":"\u4e2d\/\ud83d\ude00<&"}"#),
        ("/direct/sparse", r#"{"2":9}"#),
        ("/direct/fluent", r#"{"name":"Ada"}"#),
        ("/direct/controller", r#"{"controller":"structured"}"#),
    ] {
        let (headers, body) = send(address, "GET", path, "", b"", 200).await;
        assert_eq!(body, expected);
        assert!(headers.contains("\r\ncontent-type: application/json"));
    }
    let (headers, body) = send(address, "HEAD", "/direct/array", "", b"", 200).await;
    assert!(body.is_empty() && headers.contains("\r\ncontent-length: 7"));
    let (_, body) = send(address, "GET", "/direct/error", "", b"", 500).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["message"],
        "Server Error"
    );
    server.abort();
}

#[tokio::test]
async fn real_http_responsable_sees_the_current_request_and_uses_the_response_pipeline() {
    let (address, server) = start("controller").await;
    let (headers, body) = send(
        address,
        "GET",
        "/responsable/first?name=query",
        "",
        b"",
        200,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"label":"sync","slug":"first","input":"query"})
    );
    assert!(headers.contains("max-age=60") && headers.contains("private"));
    for (path, label) in [
        ("/responsable/async/second", "async"),
        ("/responsable/borrowed/second", "borrowed"),
    ] {
        let (_, body) = send(
            address,
            "POST",
            path,
            "application/json",
            br#"{"name":"body"}"#,
            200,
        )
        .await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"label":label,"slug":"second","input":"body"})
        );
    }
    let (_, body) = send(
        address,
        "GET",
        "/responsable/controller/third",
        "",
        b"",
        200,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"label":"controller","slug":"third","input":null})
    );
    let (_, body) = send(address, "GET", "/responsable/denied", "", b"", 404).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["message"],
        "Not Found"
    );
    let (_, body) = send(
        address,
        "HEAD",
        "/responsable/first?name=query",
        "",
        b"",
        200,
    )
    .await;
    assert!(body.is_empty());
    server.abort();
}

#[tokio::test]
async fn real_http_reads_strict_and_permissive_scalar_input() {
    let (address, server) = start("scalars").await;
    for (payload, permissive, strict) in [
        (json!({}), (42, 1.25, true), (42, 1.25, true)),
        (json!({"value": null}), (0, 0.0, false), (42, 1.25, true)),
        (json!({"value": false}), (0, 0.0, false), (42, 1.25, false)),
        (json!({"value": [1]}), (1, 1.0, false), (42, 1.25, true)),
        (json!({"value": "  01 "}), (1, 1.0, false), (42, 1.0, true)),
        (
            json!({"value": " -1.25e2tail "}),
            (-125, -125.0, false),
            (42, 1.25, true),
        ),
        (
            json!({"value": "1e-400"}),
            (0, 0.0, false),
            (42, 1.25, true),
        ),
        (
            json!({"value": "0e-400"}),
            (0, 0.0, false),
            (42, 1.25, true),
        ),
        (
            json!({"value": "  YeS  "}),
            (0, 0.0, true),
            (42, 1.25, true),
        ),
        (
            json!({"value": "  OFF  "}),
            (0, 0.0, false),
            (42, 1.25, false),
        ),
        (json!({"value": 1.0}), (1, 1.0, true), (1, 1.0, true)),
        (
            json!({"value": 1e14}),
            (100_000_000_000_000, 1e14, false),
            (42, 1e14, true),
        ),
        (
            json!({"value": "9223372036854775808"}),
            (i64::MAX, 9_223_372_036_854_775_808.0, false),
            (42, 9_223_372_036_854_775_808.0, true),
        ),
    ] {
        let bytes = serde_json::to_vec(&payload).unwrap();
        let (_, body) = send(address, "POST", "/scalars", "application/json", &bytes, 200).await;
        let response: Value = serde_json::from_str(&body).unwrap();
        // JSON 编码不会保留整数浮点数的小数点；分别验证标量值，编码字节由独立参考用例验证。
        for (actual, (integer, float, boolean)) in response
            .as_array()
            .unwrap()
            .iter()
            .zip([permissive, strict])
        {
            assert_eq!(actual["integer"].as_i64(), Some(integer), "{payload}");
            assert_eq!(actual["float"].as_f64(), Some(float), "{payload}");
            assert_eq!(actual["boolean"].as_bool(), Some(boolean), "{payload}");
        }
        assert_eq!(response.as_array().unwrap().len(), 2);
    }
    let (_, body) = send(address, "GET", "/scalars?value=19", "", b"", 200).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()[1]["integer"],
        19
    );
    let (_, body) = send(
        address,
        "POST",
        "/scalars?value=19",
        "application/x-www-form-urlencoded",
        b"value=2.5",
        200,
    )
    .await;
    let response: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(response[0]["integer"], 2);
    assert_eq!(response[1]["integer"], 42);
    assert_eq!(response[1]["float"], 2.5);
    send(
        address,
        "POST",
        "/scalars?value=19",
        "application/json",
        b"{broken",
        400,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn real_http_reads_fluent_strings_arrays_and_backed_enums() {
    let (address, server) = start("typed-values").await;
    let path = "/typed-values?label=query&stage=draft";
    let (_, body) = send(address, "POST", path, "application/json", br#"{"label":"  published  ","items":{"z":9,"0":1,"a":2},"stage":"ready","toggle":"1.0","list":["ready","unknown","draft","ready"]}"#, 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"text":"published", "loose":"published", "items":{"z":9,"0":1,"a":2}, "collected":[["z",9],[0,1],["a",2]], "stage":"Ready", "toggle":"On", "cases":[[0,"Ready"],[2,"Draft"],[3,"Ready"]]})
    );

    let (_, body) = send(address, "POST", path, "application/json", br#"{"label":null,"items":false,"stage":"none","toggle":false,"list":{"first":"draft","skip":"unknown","again":"draft"}}"#, 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"text":"fallback", "loose":"", "items":[false], "collected":[[0,false]], "stage":"Draft", "toggle":"Off", "cases":[["first","Draft"],["again","Draft"]]})
    );

    let (_, body) = send(
        address,
        "POST",
        "/typed-values",
        "application/json",
        b"{}",
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"text":"fallback", "loose":"fallback", "items":[], "collected":[], "stage":"Draft", "toggle":null, "cases":[]})
    );

    for invalid in [
        br#"{"stage":[]}"#.as_slice(),
        br#"{"toggle":"1tail"}"#,
        br#"{"list":["ready",{}]}"#,
    ] {
        send(address, "POST", path, "application/json", invalid, 400).await;
    }
    server.abort();
}

#[tokio::test]
async fn real_http_reads_dates_and_selects_named_input_paths() {
    let (address, server) = start("dated-values").await;
    let (_, body) = send(address, "POST", "/dated-values?query=keep", "application/json", br#"{"when":"2026-01-02T03:04:05.123456+08:00","formatted":"03/01/2026 04:05:06.765432","user":{"name":"Ada","note":null,"secret":"remove"}}"#, 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value["dates"][0],
        json!([
            "2026-01-02 03:04:05.123456 +08:00",
            "+08:00",
            123_456,
            1_767_294_245_i64
        ])
    );
    assert_eq!(
        value["dates"][1],
        json!([
            "2026-01-03 04:05:06.765432 +09:00",
            "Asia/Tokyo",
            765_432,
            1_767_380_706_i64
        ])
    );
    let selected = json!({"user":{"name":"Ada","note":null},"query":"keep"});
    assert_eq!(value["selected"], selected);
    assert_eq!(value["array"], selected);
    assert_eq!(value["remainder"], selected);
    assert_eq!(
        value["collection"],
        json!([["user",{"name":"Ada","note":null}],["query","keep"]])
    );

    for (payload, expected) in [
        (
            br#"{"when":"2026-01-02 +1 day"}"#.as_slice(),
            json!([
                "2026-01-03 00:00:00.000000 +00:00",
                "UTC",
                0,
                1_767_398_400_i64
            ]),
        ),
        (
            br#"{"when":-0.25}"#,
            json!(["1969-12-31 23:59:59.750000 +00:00", "+00:00", 750_000, -1]),
        ),
        (br#"{"when":"   ","formatted":null}"#, Value::Null),
    ] {
        let (_, body) = send(
            address,
            "POST",
            "/dated-values",
            "application/json",
            payload,
            200,
        )
        .await;
        let date: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(date["dates"], json!([expected, null]));
    }
    for invalid in [
        br#"{"when":"not a date"}"#.as_slice(),
        br#"{"formatted":"wrong format"}"#,
    ] {
        send(
            address,
            "POST",
            "/dated-values",
            "application/json",
            invalid,
            400,
        )
        .await;
    }
    server.abort();
}

#[tokio::test]
async fn real_http_reads_calendar_intervals_and_explicit_units() {
    let (address, server) = start("intervals").await;
    for (body, expected) in [
        (
            br#"{"duration":"P1Y2M3DT4H5M6.123456S","seconds":"90.125suffix"}"#.as_slice(),
            json!([
                [1, 2, 3, 4, 5, 6, 123456, false, null],
                [0, 0, 0, 0, 0, 90, 125000, false, null]
            ]),
        ),
        (
            br#"{"duration":"2 hours 30 minutes","seconds":-1.25}"#,
            json!([
                [0, 0, 0, 2, 30, 0, 0, false, null],
                [0, 0, 0, 0, 0, -1, -250000, false, null]
            ]),
        ),
        (
            br#"{"duration":"2 days ago","seconds":" "}"#,
            json!([[0, 0, -2, 0, 0, 0, 0, false, null], null]),
        ),
        (br#"{"duration":3,"seconds":null}"#, json!([null, null])),
        (b"{}", json!([null, null])),
    ] {
        let path = if body == b"{}" {
            "/intervals"
        } else {
            "/intervals?duration=4%20hours"
        };
        let (_, actual) = send(address, "POST", path, "application/json", body, 200).await;
        assert_eq!(serde_json::from_str::<Value>(&actual).unwrap(), expected);
    }
    for body in [
        br#"{"duration":"P1Q"}"#.as_slice(),
        br#"{"invalid_unit":10}"#,
    ] {
        send(address, "POST", "/intervals", "application/json", body, 400).await;
    }
    server.abort();
}

#[tokio::test]
async fn real_http_dispatches_resource_and_api_resource_actions() {
    let (address, server) = start("resource").await;
    for (method, path, expected) in [
        (
            "GET",
            "/photos",
            json!(["resource", "index", null, null, null]),
        ),
        (
            "GET",
            "/photos/create",
            json!(["resource", "create", null, null, null]),
        ),
        (
            "POST",
            "/photos",
            json!(["resource", "store", null, null, null]),
        ),
        (
            "GET",
            "/photos/2",
            json!(["resource", "member", "2", null, null]),
        ),
        (
            "GET",
            "/photos/2/edit",
            json!(["resource", "edit", "2", null, null]),
        ),
        (
            "PUT",
            "/photos/2",
            json!(["resource", "update", "2", null, null]),
        ),
        (
            "PATCH",
            "/photos/2",
            json!(["resource", "update", "2", null, null]),
        ),
        (
            "DELETE",
            "/photos/2",
            json!(["resource", "destroy", "2", null, null]),
        ),
        (
            "GET",
            "/api/albums/7/photos",
            json!(["resource", "index", null, "7", null]),
        ),
        (
            "POST",
            "/api/albums/7/photos",
            json!(["resource", "store", null, "7", null]),
        ),
        (
            "GET",
            "/api/photos/3",
            json!(["resource", "member", null, null, "3"]),
        ),
        (
            "PATCH",
            "/api/photos/3",
            json!(["resource", "update", null, null, "3"]),
        ),
    ] {
        let (_, body) = send(address, method, path, "", b"", 200).await;
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), expected);
    }
    for path in [
        "/api/albums/7/photos/create",
        "/api/photos/3/edit",
        "/api/albums/7/photos/3",
        "/api/photos/create",
    ] {
        send(address, "GET", path, "", b"", 404).await;
    }
    send(address, "DELETE", "/api/photos/3", "", b"", 405).await;
    server.abort();
}

#[test]
fn resource_registration_preserves_names_binding_fields_and_atomic_failure() {
    let mut router = Router::new();
    router.resource("photos", photo_resource()).unwrap();
    let original = router.get_routes();
    assert_eq!(original.len(), 7);
    assert!(
        original
            .iter()
            .any(|route| route.name.as_deref() == Some("photos.show")
                && route.action.ends_with("EchoController@show"))
    );
    assert!(matches!(
        router.resource(
            "photos",
            ResourceController::new().action(ResourceAction::Index, EchoController::index),
        ),
        Err(RoutingError::MissingResourceAction {
            action: "create",
            ..
        })
    ));
    assert_eq!(router.get_routes(), original);
    assert!(
        router
            .resource(
                "photos",
                photo_resource().configure(|action, _| {
                    if action == ResourceAction::Store {
                        return Err(RoutingError::InvalidAction);
                    }
                    Ok(())
                }),
            )
            .is_err()
    );
    assert_eq!(router.get_routes(), original);

    router
        .api_resource(
            "albums.photos",
            photo_resource()
                .only(&[ResourceAction::Index, ResourceAction::Show])
                .shallow()
                .parameters(&[("photos", "image")])
                .scoped(&[("image", "slug")])
                .names(&[(ResourceAction::Show, "pictures.read")]),
        )
        .unwrap();
    let routes = router.get_routes();
    assert!(
        routes
            .iter()
            .any(|route| route.name.as_deref() == Some("albums.photos.index")
                && route.uri == "albums/{album}/photos")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.name.as_deref() == Some("pictures.read")
                && route.uri == "photos/{image:slug}")
    );
    router
        .api_resource(
            "api/forms",
            photo_resource().only(&[ResourceAction::Create]),
        )
        .unwrap();
    assert!(
        router
            .get_routes()
            .iter()
            .any(|route| route.method == "GET|HEAD"
                && route.uri == "api/forms/create"
                && route.name.as_deref() == Some("forms.create"))
    );
}

#[tokio::test]
async fn real_http_dispatches_singletons_with_lifecycle_and_shallow_paths() {
    let (address, server) = start("singleton").await;
    for (method, path, action, parent) in [
        ("GET", "/profile", "member", None),
        ("GET", "/profile/edit", "edit", None),
        ("PUT", "/profile", "update", None),
        ("PATCH", "/profile", "update", None),
        ("DELETE", "/preferences", "destroy", None),
        ("GET", "/api/albums/7/thumbnail", "member", Some("7")),
        ("POST", "/api/albums/7/thumbnail", "store", Some("7")),
        ("PATCH", "/api/thumbnail", "update", None),
        ("DELETE", "/api/thumbnail", "destroy", None),
    ] {
        let (_, body) = send(address, method, path, "", b"", 200).await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!(["singleton", action, parent, null, null])
        );
    }
    for (method, path, status) in [
        ("POST", "/profile", 405),
        ("DELETE", "/profile", 405),
        ("POST", "/preferences", 405),
        ("GET", "/profile/3", 404),
        ("GET", "/profile/create", 404),
        ("GET", "/api/albums/7/thumbnail/create", 404),
        ("GET", "/api/thumbnail/edit", 404),
        ("GET", "/api/thumbnail", 405),
        ("PATCH", "/api/albums/7/thumbnail", 405),
    ] {
        send(address, method, path, "", b"", status).await;
    }
    server.abort();
}

#[test]
fn singleton_routes_apply_explicit_selection_and_keep_binding_metadata() {
    let mut router = Router::new();
    router
        .api_singleton(
            "pages.summary",
            photo_resource()
                .only(&[ResourceAction::Edit])
                .names(&[(ResourceAction::Edit, "summary.change")]),
        )
        .unwrap();
    let routes = router.get_routes();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].method, "GET|HEAD");
    assert_eq!(routes[0].uri, "pages/{page}/summary/edit");
    assert_eq!(routes[0].name.as_deref(), Some("summary.change"));
    router
        .singleton(
            "profiles.card",
            photo_resource()
                .creatable()
                .destroyable()
                .scoped(&[("profile", "slug")]),
        )
        .unwrap();
    let routes = router.get_routes();
    assert_eq!(routes.len(), 7);
    for (method, path) in [
        ("GET|HEAD", "profiles/{profile:slug}/card/create"),
        ("POST", "profiles/{profile:slug}/card"),
        ("DELETE", "profiles/{profile:slug}/card"),
    ] {
        assert!(
            routes
                .iter()
                .any(|route| route.method == method && route.uri == path),
            "{method} {path}"
        );
    }
}

fn chunked_content(mut body: &str) -> String {
    let mut result = String::new();
    loop {
        let (length, next) = body.split_once("\r\n").unwrap();
        let length = usize::from_str_radix(length, 16).unwrap();
        if length == 0 {
            return result;
        }
        result.push_str(&next[..length]);
        body = next[length..].strip_prefix("\r\n").unwrap();
    }
}

#[tokio::test]
async fn real_http_streams_json_events_and_downloads() {
    let (address, server) = start("streams").await;
    let (headers, body) = send(address, "GET", "/stream.json", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-type: application/json"));
    assert!(headers.contains("\r\ntransfer-encoding: chunked"));
    assert_eq!(
        serde_json::from_str::<Value>(&chunked_content(&body)).unwrap(),
        json!({"users":[1,2],"counts":{"active":2},"<é&":"<é>/\"&'😀\u{2028}","meta":["ready",[]]})
    );
    assert!(chunked_content(&body).contains(
        r#""\u003C\u00e9\u0026":"\u003C\u00e9\u003E\/\u0022\u0026\u0027\ud83d\ude00\u2028""#,
    ));
    let (_, body) = send(address, "HEAD", "/stream.json", "", b"", 200).await;
    assert!(body.is_empty());

    let (headers, body) = send(address, "GET", "/stream.events", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-type: text/event-stream; charset=utf-8"));
    assert!(headers.contains("\r\ncache-control: no-cache, private"));
    assert!(headers.contains("\r\nx-accel-buffering: no"));
    assert_eq!(
        chunked_content(&body),
        concat!(
            "event: update\ndata: line\ndata: next\ndata: final\n\nevent: snapshot\ndata: ",
            r#"{"ready":true,"text":"\u003Cé\u003E\/\u0022\u0026\u0027😀\u2028"}"#,
            "\n\nevent: update\ndata: </stream>\n\n"
        )
    );

    let (headers, body) = send(address, "GET", "/stream.download", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-disposition: attachment; filename=resume.txt; filename*=utf-8''r%C3%A9sum%C3%A9.txt"));
    assert_eq!(chunked_content(&body), "firstsecond");
    server.abort();
}

#[tokio::test]
async fn response_streams_preserve_failure_and_lazy_sources() {
    use futures_util::{StreamExt as _, stream};

    let (release, ready) = tokio::sync::oneshot::channel::<()>();
    let source = stream::once(async {
        ready.await.unwrap();
        Ok::<_, std::io::Error>(1)
    });
    let mut response = Response::stream_json(StatusCode::OK, StreamedJson::items(source));
    let mut body = response.take_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(1), body.next()).await;
    assert_eq!(first.unwrap().unwrap().unwrap().as_ref(), b"[");
    release.send(()).unwrap();
    assert_eq!(body.next().await.unwrap().unwrap().as_ref(), b"1");
    assert_eq!(body.next().await.unwrap().unwrap().as_ref(), b"]");

    let source = stream::iter([Err::<StreamedEvent, _>(std::io::Error::other(
        "ended early",
    ))]);
    let mut response = Response::event_stream(source);
    let mut body = response.take_body().into_data_stream();
    assert!(body.next().await.unwrap().is_err());
    assert!(body.next().await.is_none());

    let empty = || stream::empty::<Result<StreamedEvent, std::io::Error>>();
    let mut response = Response::event_stream_with_end(empty(), None);
    assert!(
        response
            .take_body()
            .into_data_stream()
            .next()
            .await
            .is_none()
    );
    let end = StreamedEvent::new("done", "ok").unwrap();
    let mut response = Response::event_stream_with_end(empty(), Some(end));
    let mut body = response.take_body().into_data_stream();
    assert_eq!(
        body.next().await.unwrap().unwrap().as_ref(),
        b"event: done\ndata: ok\n\n"
    );
    assert!(body.next().await.is_none());

    assert!(StreamedEvent::new("injected\nevent", "data").is_err());
    assert!(
        Response::stream_download(
            stream::empty::<Result<String, std::io::Error>>(),
            Some("../unsafe"),
            HeaderMap::new(),
            "attachment"
        )
        .is_err()
    );
}

#[tokio::test]
async fn real_http_removes_headers_and_applies_cache_validators() {
    let (address, server) = start("cache-headers").await;
    let (headers, body) = send(address, "GET", "/cacheable", "", b"", 200).await;
    // Fixed the framework / PHP 8.5 hash('xxh128', 'cache me').
    let tag = "\"a9c4c77c2e3f060abb5bb0773cfe00bf\"";
    assert_eq!(body, "cache me");
    assert!(headers.contains("\r\ncache-control: max-age=2628000, public"));
    assert!(headers.contains(&format!("\r\netag: {tag}")));
    assert!(headers.contains("\r\nlast-modified: Mon, 01 Jan 2024 00:00:00 GMT"));
    assert!(!headers.contains("\r\nx-debug:") && !headers.contains("\r\nx-remove:"));

    let fields = format!("If-None-Match: \"other,tag\"\r\nIf-None-Match: W/{tag}\r\n");
    let (headers, body) =
        send_with_headers(address, "HEAD", "/cacheable", "", b"", 304, &fields).await;
    assert!(body.is_empty());
    assert!(headers.contains(&format!("\r\netag: {tag}")));
    assert!(!headers.contains("\r\nlast-modified:") && !headers.contains("\r\ncontent-type:"));
    let (_, body) = send_with_headers(
        address,
        "GET",
        "/cacheable",
        "",
        b"",
        200,
        "If-None-Match: \"different\"\r\nIf-Modified-Since: Mon, 01 Jan 2024 00:00:00 GMT\r\n",
    )
    .await;
    assert_eq!(body, "cache me");

    let (headers, body) = send_with_headers(
        address,
        "GET",
        "/cache-date",
        "",
        b"",
        304,
        "If-Modified-Since: Mon, 01 Jan 2024 00:00:00 GMT\r\n",
    )
    .await;
    assert!(body.is_empty());
    assert!(headers.contains("\r\ncache-control: private, must-revalidate"));

    let (headers, body) = send(address, "GET", "/cache-stream", "", b"", 200).await;
    assert!(headers.contains("\r\ncache-control: public, s-maxage=600"));
    assert!(!headers.contains("\r\netag:"));
    assert_eq!(chunked_content(&body), "firstsecond");

    let (headers, body) = send_with_headers(
        address,
        "GET",
        "/cache-manual",
        "",
        b"",
        304,
        "If-None-Match: *\r\n",
    )
    .await;
    assert!(body.is_empty());
    assert!(headers.contains("\r\netag: \"manual\""));
    assert!(headers.contains("\r\ncache-control: ext=\"a,b\", private, s-maxage=30"));

    for (method, path, status) in [
        ("POST", "/cacheable", 200),
        ("GET", "/cache-invalid-status", 404),
    ] {
        let (headers, _) = send(address, method, path, "", b"", status).await;
        assert!(!headers.contains("\r\ncache-control:") && !headers.contains("\r\netag:"));
    }
    server.abort();
}

#[tokio::test]
async fn real_http_serves_jsonp_markdown_empty_and_view_responses() {
    let root = tempfile::tempdir().unwrap();
    let views = root.path().join("resources/views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("answer.blade.html"), "Hello {{ $name }}!").unwrap();
    let (address, server) = start_at("response-factory", root.path().into()).await;

    let (headers, body) = send(
        address,
        "GET",
        "/response.jsonp?callback=app.handlers%5B2%5D",
        "",
        b"",
        200,
    )
    .await;
    assert!(headers.contains("\r\ncontent-type: text/javascript; charset=utf-8"));
    assert_eq!(
        body,
        r#"/**/app.handlers[2]("caf\u00e9 \/ \ud83d\ude00 <\/script>");"#
    );
    let (_, body) = send(
        address,
        "GET",
        "/response.jsonp?callback=%E5%BA%94%E7%AD%94%5B%22done%22%5D",
        "",
        b"",
        200,
    )
    .await;
    assert!(body.starts_with("/**/应答[\"done\"]("));
    let (headers, body) = send(address, "GET", "/response.jsonp", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-type: application/json"));
    assert_eq!(
        serde_json::from_str::<String>(&body).unwrap(),
        "café / 😀 </script>"
    );
    for path in [
        "/response.jsonp?callback=alert%281%29",
        "/response.jsonp?callback=class",
        "/response.jsonp?callback=va%0alid",
        "/response.jsonp?callback%5B%5D=valid",
    ] {
        send(address, "GET", path, "", b"", 400).await;
    }

    let (headers, body) = send(address, "GET", "/response.md", "", b"", 202).await;
    assert!(headers.contains("\r\ncontent-type: text/markdown; charset=utf-8"));
    assert_eq!(body, "# Heading\n");
    let (_, body) = send(address, "HEAD", "/response.md", "", b"", 202).await;
    assert!(body.is_empty());
    let (headers, body) = send(address, "GET", "/response.md?empty=1", "", b"", 304).await;
    assert!(!headers.contains("\r\ncontent-type:") && body.is_empty());

    let (headers, body) = send(address, "GET", "/response-empty", "", b"", 204).await;
    assert!(headers.contains("\r\nx-probe: empty"));
    assert!(!headers.contains("\r\ncontent-type:") && body.is_empty());

    let (headers, body) = send(address, "GET", "/response-view", "", b"", 201).await;
    assert!(headers.contains("\r\nx-probe: view"));
    assert!(headers.contains("\r\ncontent-type: text/html; charset=utf-8"));
    assert_eq!(body, "Hello &lt;Ada&gt;!");
    server.abort();
}

#[tokio::test]
async fn real_http_prepares_file_responses_after_cache_middleware() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("document.txt"), b"0123456789").unwrap();
    std::fs::write(root.path().join("empty.bin"), b"").unwrap();
    let (address, server) = start_at("binary-file", root.path().into()).await;

    let (headers, body) = send(address, "GET", "/file", "", b"", 200).await;
    assert_eq!(body, "0123456789");
    assert!(headers.contains("\r\ncontent-length: 10"));
    assert!(headers.contains("\r\ncontent-type: text/plain; charset=utf-8"));
    assert!(headers.contains("\r\naccept-ranges: bytes"));
    assert!(headers.contains("\r\ncache-control: public"));
    assert!(!headers.contains("\r\ncontent-disposition:"));
    let modified = headers
        .lines()
        .find_map(|line| line.strip_prefix("last-modified: "))
        .unwrap();

    for (method, fields, status, expected, content_range) in [
        (
            "GET",
            "Range: bytes=2-4\r\n",
            206,
            "234",
            Some("bytes 2-4/10"),
        ),
        (
            "GET",
            "Range: bytes=7-\r\n",
            206,
            "789",
            Some("bytes 7-9/10"),
        ),
        (
            "GET",
            "Range: bytes=-2\r\n",
            206,
            "89",
            Some("bytes 8-9/10"),
        ),
        ("GET", "Range: bytes=12-15\r\n", 416, "", Some("bytes */10")),
        ("GET", "Range: bytes=0-1,5-6\r\n", 200, "0123456789", None),
        ("GET", "Range: bytes=bad-9\r\n", 200, "0123456789", None),
        ("HEAD", "Range: bytes=0-1\r\n", 200, "", None),
        (
            "GET",
            "Range: bytes=0-1\r\nIf-Range: \"stale\"\r\n",
            200,
            "0123456789",
            None,
        ),
    ] {
        let (headers, body) =
            send_with_headers(address, method, "/file", "", b"", status, fields).await;
        assert_eq!(body, expected, "{method} {fields}");
        let actual = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-range: "));
        assert_eq!(actual, content_range);
        let length = if method == "HEAD" { 10 } else { expected.len() };
        assert!(
            headers
                .lines()
                .any(|line| line == format!("content-length: {length}"))
        );
    }
    let (_, body) = send_with_headers(
        address,
        "GET",
        "/file",
        "",
        b"",
        206,
        &format!("Range: bytes=0-1\r\nIf-Range: {modified}\r\n"),
    )
    .await;
    assert_eq!(body, "01");

    let (headers, body) = send_with_headers(
        address,
        "GET",
        "/file-cached",
        "",
        b"",
        206,
        "Range: bytes=2-3\r\nIf-Range: \"release\"\r\n",
    )
    .await;
    assert_eq!(body, "23");
    assert!(headers.contains("\r\ncache-control: max-age=60, private"));
    let (headers, body) = send_with_headers(
        address,
        "GET",
        "/file-cached",
        "",
        b"",
        304,
        "Range: bytes=50-70\r\nIf-None-Match: W/\"release\"\r\n",
    )
    .await;
    assert!(body.is_empty() && !headers.contains("\r\ncontent-range:"));
    assert!(headers.contains("\r\netag: \"release\""));

    let (headers, body) = send(address, "GET", "/download", "", b"", 200).await;
    assert_eq!(body, "0123456789");
    assert!(headers.contains("\r\ncontent-disposition: attachment; filename=resume.txt; filename*=utf-8''r%C3%A9sum%C3%A9.txt"));
    let (headers, _) = send(address, "HEAD", "/download-default", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-disposition: inline; filename=document.txt"));

    let (headers, body) = send(address, "GET", "/file-zero", "", b"", 200).await;
    assert!(body.is_empty() && headers.contains("\r\ncontent-length: 0"));
    send(address, "GET", "/file-missing", "", b"", 500).await;
    server.abort();
}

#[tokio::test]
async fn real_http_dispatches_provider_macros_in_each_application() {
    let ((first, first_server), (second, second_server)) =
        tokio::join!(start("macro-first"), start("macro-second"));
    let (left, right) = tokio::join!(
        send(first, "GET", "/macro?value=caf%C3%A9", "", b"", 201),
        send(second, "GET", "/macro?value=hello", "", b"", 201),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&left.1).unwrap(),
        json!(["macro-first", "CAFÉ"])
    );
    assert_eq!(
        serde_json::from_str::<Value>(&right.1).unwrap(),
        json!(["macro-second", "HELLO"])
    );

    let (_, body) = send(first, "GET", "/macro-replace", "", b"", 200).await;
    assert_eq!(body, "\"replaced\"");
    send(first, "GET", "/macro", "", b"", 400).await;
    send(first, "POST", "/macro-flush", "", b"", 204).await;
    let (_, body) = send(first, "GET", "/macro-present", "", b"", 200).await;
    assert_eq!(body, "false");
    send(first, "GET", "/macro?value=gone", "", b"", 500).await;
    let (_, body) = send(second, "GET", "/macro?value=still", "", b"", 201).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!(["macro-second", "STILL"])
    );
    first_server.abort();
    second_server.abort();
}

#[tokio::test]
async fn sse_reports_source_errors_in_the_originating_application() {
    use openrust::{foundation::configuration::ReportDecision, support::facades::Config};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let reports = Arc::new(AtomicUsize::new(0));
    let observed = reports.clone();
    let (address, server) = start_with("stream-failure", std::env::temp_dir(), |builder| {
        builder.with_exceptions(move |exceptions| {
            exceptions.report::<std::io::Error>(move |error| {
                assert_eq!(error.to_string(), "sse fixture interruption");
                assert!(Config::get("probe.file_root").unwrap().is_some());
                observed.fetch_add(1, Ordering::Relaxed);
                ReportDecision::Stop
            });
        })
    })
    .await;
    let (_, body) = send(address, "GET", "/stream.events?fail=1", "", b"", 200).await;
    assert_eq!(
        chunked_content(&body),
        "event: update\ndata: before failure\n\n"
    );
    assert_eq!(reports.load(Ordering::Relaxed), 1);
    server.abort();
}
