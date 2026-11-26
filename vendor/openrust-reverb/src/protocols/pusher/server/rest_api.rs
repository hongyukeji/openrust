use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Path, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{App, ServerState, debug_channel, signatures};
use crate::protocols::pusher::{
    channels::{ChannelMetrics, Metrics, valid_delivery_channel},
    scaling::Delivery,
};

pub(super) fn routes(prefix: &str) -> Router<Arc<ServerState>> {
    let app = format!("{prefix}/apps/{{id}}");
    Router::new()
        .route(
            &format!("{prefix}/up"),
            get(|| async { Json(json!({"health":"OK"})) }),
        )
        .route(&format!("{app}/events"), post(event))
        .route(&format!("{app}/batch_events"), post(batch))
        .route(&format!("{app}/connections"), get(connections))
        .route(&format!("{app}/channels"), get(channels))
        .route(&format!("{app}/channels/{{channel}}"), get(channel))
        .route(&format!("{app}/channels/{{channel}}/users"), get(users))
}

struct SignedRequest {
    state: Arc<ServerState>,
    app: Arc<App>,
    channel: Option<String>,
    query: BTreeMap<String, String>,
    body: Bytes,
}

impl FromRequest<Arc<ServerState>> for SignedRequest {
    type Rejection = Response;

    async fn from_request(request: Request, state: &Arc<ServerState>) -> Result<Self, Response> {
        let (mut parts, body) = request.into_parts();
        let Path(mut parameters) =
            Path::<HashMap<String, String>>::from_request_parts(&mut parts, state)
                .await
                .map_err(IntoResponse::into_response)?;
        let app = parameters
            .remove("id")
            .and_then(|id| state.ids.get(&id))
            .cloned()
            .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;
        let method = parts.method.clone();
        let uri = parts.uri.clone();
        let body = Bytes::from_request(Request::from_parts(parts, body), state)
            .await
            .map_err(IntoResponse::into_response)?;
        let mut query = BTreeMap::new();
        for (key, value) in url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
        {
            if query.insert(key.into_owned(), value.into_owned()).is_some() {
                return Err(StatusCode::BAD_REQUEST.into_response());
            }
        }
        let path = uri.path().strip_prefix(&state.prefix).unwrap_or(uri.path());
        if query.get("auth_key") != Some(&app.configuration.key)
            || !signatures::verify_request(
                app.configuration.secret.as_bytes(),
                method.as_str(),
                path,
                &query,
                &body,
            )
        {
            return Err(StatusCode::UNAUTHORIZED.into_response());
        }
        Ok(Self {
            state: state.clone(),
            app,
            channel: parameters.remove("channel"),
            query,
            body,
        })
    }
}

impl SignedRequest {
    async fn metrics(
        &self,
        selected: &[String],
        prefix: Option<&str>,
    ) -> Result<Metrics, StatusCode> {
        if let Some(publisher) = &self.state.publisher {
            publisher
                .metrics(&self.app.configuration.id, selected, prefix)
                .await
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
        } else {
            Ok(self.app.broker.metrics(selected, prefix))
        }
    }

    async fn dispatch(&self, deliveries: Vec<Delivery>) -> Result<(), StatusCode> {
        for delivery in deliveries {
            if self.state.debug {
                let channels: Vec<_> = delivery
                    .channels
                    .iter()
                    .map(|channel| debug_channel(channel))
                    .collect();
                eprintln!(
                    "[reverb] publishing app={:?} event={:?} channels={:?}",
                    delivery.app_id, delivery.event, channels
                );
            }
            if let Some(publisher) = &self.state.publisher {
                publisher
                    .publish(&delivery)
                    .await
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            } else {
                self.state.deliver(delivery);
            }
        }
        Ok(())
    }

    fn info(&self) -> &str {
        self.query.get("info").map_or("", String::as_str)
    }
}

#[derive(Deserialize)]
struct EventInput {
    name: String,
    data: String,
    socket_id: Option<String>,
    info: Option<String>,
}

impl EventInput {
    fn into_delivery(
        self,
        app: &App,
        mut channels: Vec<String>,
    ) -> Result<(Delivery, Option<String>), StatusCode> {
        if self.name.trim().is_empty()
            || self.data.trim().is_empty()
            || channels.is_empty()
            || channels.iter().any(|name| !valid_delivery_channel(name))
        {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        channels.sort_unstable();
        channels.dedup();
        Ok((
            Delivery {
                app_id: app.configuration.id.clone(),
                channels,
                event: self.name,
                data: self.data,
                excluded: self.socket_id,
                user_id: None,
            },
            self.info,
        ))
    }
}

#[derive(Deserialize)]
struct Single {
    #[serde(flatten)]
    event: EventInput,
    channel: Option<String>,
    channels: Option<Vec<String>>,
}

async fn event(request: SignedRequest) -> Result<Json<Value>, StatusCode> {
    let event: Single =
        serde_json::from_slice(&request.body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let channels = event
        .channels
        .filter(|channels| !channels.is_empty())
        .unwrap_or_else(|| event.channel.into_iter().collect());
    let (delivery, info) = event.event.into_delivery(&request.app, channels)?;
    let channels = delivery.channels.clone();
    request.dispatch(vec![delivery]).await?;
    let Some(info) = info else {
        return Ok(Json(json!({})));
    };
    let metrics = request.metrics(&channels, None).await?;
    let info = channels
        .into_iter()
        .map(|name| {
            let fields = channel_info(&name, metrics.channels.get(&name), &info, false);
            (name, fields)
        })
        .collect::<Map<_, _>>();
    Ok(Json(json!({"channels": info})))
}

#[derive(Deserialize)]
struct Batch {
    batch: Vec<BatchItem>,
}

#[derive(Deserialize)]
struct BatchItem {
    #[serde(flatten)]
    event: EventInput,
    channel: String,
}

async fn batch(request: SignedRequest) -> Result<Json<Value>, StatusCode> {
    let batch: Batch =
        serde_json::from_slice(&request.body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if batch.batch.is_empty() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut deliveries = Vec::with_capacity(batch.batch.len());
    let mut fields = Vec::with_capacity(batch.batch.len());
    for item in batch.batch {
        let (delivery, info) = item
            .event
            .into_delivery(&request.app, vec![item.channel.clone()])?;
        deliveries.push(delivery);
        fields.push((item.channel, info));
    }
    request.dispatch(deliveries).await?;
    let selected: Vec<_> = fields
        .iter()
        .filter(|(_, info)| info.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    if selected.is_empty() {
        return Ok(Json(json!({"batch": {}})));
    }
    let metrics = request.metrics(&selected, None).await?;
    let batch: Vec<_> = fields
        .into_iter()
        .map(|(name, info)| {
            info.map_or_else(
                || json!({}),
                |info| channel_info(&name, metrics.channels.get(&name), &info, false),
            )
        })
        .collect();
    Ok(Json(json!({"batch": batch})))
}

async fn connections(request: SignedRequest) -> Result<Json<Value>, StatusCode> {
    Ok(Json(
        json!({"connections": request.metrics(&[], None).await?.connections}),
    ))
}

async fn channels(request: SignedRequest) -> Result<Json<Value>, StatusCode> {
    let prefix = request.query.get("filter_by_prefix").map(String::as_str);
    let metrics = request.metrics(&[], prefix).await?;
    let info = metrics
        .channels
        .iter()
        .map(|(name, snapshot)| {
            (
                name.clone(),
                channel_info(name, Some(snapshot), request.info(), false),
            )
        })
        .collect::<Map<_, _>>();
    Ok(Json(json!({"channels": info})))
}

async fn channel(request: SignedRequest) -> Result<Json<Value>, StatusCode> {
    let name = request.channel.clone().ok_or(StatusCode::BAD_REQUEST)?;
    let metrics = request.metrics(std::slice::from_ref(&name), None).await?;
    Ok(Json(channel_info(
        &name,
        metrics.channels.get(&name),
        request.info(),
        true,
    )))
}

async fn users(request: SignedRequest) -> Result<Json<Value>, StatusCode> {
    let name = request.channel.clone().ok_or(StatusCode::BAD_REQUEST)?;
    let metrics = request.metrics(std::slice::from_ref(&name), None).await?;
    let channel = metrics.channels.get(&name).ok_or(StatusCode::NOT_FOUND)?;
    if !name.starts_with("presence-") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let users: Vec<_> = channel.users.iter().map(|id| json!({"id": id})).collect();
    Ok(Json(json!({"users": users})))
}

fn channel_info(
    name: &str,
    snapshot: Option<&ChannelMetrics>,
    info: &str,
    occupied: bool,
) -> Value {
    let requested = |field| info.split(',').any(|name| name == field);
    let mut result = Map::new();
    if occupied || requested("occupied") {
        result.insert("occupied".into(), json!(snapshot.is_some()));
    }
    if let Some(snapshot) = snapshot {
        if name.starts_with("presence-") {
            if requested("user_count") {
                result.insert("user_count".into(), json!(snapshot.users.len()));
            }
        } else if requested("subscription_count") {
            result.insert("subscription_count".into(), json!(snapshot.subscriptions));
        }
        if let Some(cache) = &snapshot.cache
            && requested("cache")
        {
            result.insert("cache".into(), json!(cache));
        }
    }
    Value::Object(result)
}
