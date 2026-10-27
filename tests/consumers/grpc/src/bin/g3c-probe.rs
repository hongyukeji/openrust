use std::{collections::BTreeSet, env, error::Error, time::Duration};

use openrust::{
    config::{FrameworkConfig, GrpcClientConfig, GrpcConfig},
    core::{AppBuilder, finalize},
    grpc::{GrpcClientRegistry, GrpcPlugin, health_pb, tonic},
};
use tonic::Request;

pub mod proto {
    tonic::include_proto!("openrust.example.v1");
}

use proto::{EchoRequest, echo_client::EchoClient};

async fn channel() -> Result<tonic::transport::Channel, Box<dyn Error>> {
    let endpoint = env::var("G3C_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50070".to_owned());
    let config = FrameworkConfig {
        grpc: GrpcConfig {
            clients: std::collections::BTreeMap::from([(
                "g3c".to_owned(),
                GrpcClientConfig {
                    endpoint,
                    tls: None,
                },
            )]),
            default_client: Some("g3c".to_owned()),
            ..GrpcConfig::default()
        },
        ..FrameworkConfig::default()
    };
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    builder.plugin(GrpcPlugin::new());
    let app = finalize(builder).await?;
    let clients = app.state().try_get::<GrpcClientRegistry>()?;
    let channel = clients.channel(None)?;
    Ok(channel)
}

async fn client() -> Result<EchoClient<tonic::transport::Channel>, Box<dyn Error>> {
    Ok(EchoClient::new(channel().await?))
}

fn with_run_id<T>(request: &mut Request<T>) -> Result<(), Box<dyn Error>> {
    let run_id = env::var("G3C_RUN_ID").unwrap_or_else(|_| "unspecified".to_owned());
    request.metadata_mut().insert("x-g3c-run", run_id.parse()?);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mode = env::args().nth(1).ok_or("缺少探针模式")?;
    let mut client = client().await?;
    match mode.as_str() {
        "distribution" => {
            let mut nodes = BTreeSet::new();
            for index in 0..40 {
                let mut request = Request::new(EchoRequest {
                    value: index.to_string(),
                    client_hint: String::new(),
                });
                with_run_id(&mut request)?;
                let response = client.unary(request).await?.into_inner();
                println!("RPC_NODE={}", response.api_revision);
                nodes.insert(response.api_revision);
            }
            if nodes != BTreeSet::from(["A".to_owned(), "B".to_owned()]) {
                return Err(format!("单一 HTTP/2 Channel 未到达两个节点：{nodes:?}").into());
            }
            println!("DISTRIBUTION_OK={nodes:?}");
        }
        "drain" => {
            let mut request = Request::new(EchoRequest {
                value: "long".to_owned(),
                client_hint: String::new(),
            });
            with_run_id(&mut request)?;
            let mut stream = client.server_stream(request).await?.into_inner();
            let first = stream.message().await?.ok_or("长流在首条消息前结束")?;
            let expected = match first.api_revision.as_str() {
                "A" => "B",
                "B" => "A",
                other => return Err(format!("未知节点身份：{other}").into()),
            };
            println!("STREAM_NODE={}", first.api_revision);
            println!("STREAM_FIRST={}", first.value);
            let interrupted = tokio::time::timeout(Duration::from_secs(20), async {
                while let Ok(Some(_)) = stream.message().await {
                }
            })
            .await;
            if interrupted.is_err() {
                return Err("节点退出后长流未在 20 秒内中断".into());
            }
            println!("STREAM_INTERRUPTED=true");
            let mut request = Request::new(EchoRequest {
                value: "reconnected".to_owned(),
                client_hint: String::new(),
            });
            with_run_id(&mut request)?;
            let response = client.unary(request).await?.into_inner();
            println!("RECONNECTED_NODE={}", response.api_revision);
            if response.api_revision != expected {
                return Err(format!(
                    "摘流后预期由节点 {expected} 响应，实际为 {}",
                    response.api_revision
                )
                .into());
            }
        }
        "survivor" => {
            let expected = env::var("G3C_EXPECTED_NODE")?;
            for _ in 0..10 {
                let response = client
                    .unary(Request::new(EchoRequest {
                        value: "health-drain".to_owned(),
                        client_hint: String::new(),
                    }))
                    .await?
                    .into_inner();
                if response.api_revision != expected {
                    return Err(format!(
                        "Envoy 在节点摘流期间将 RPC 发给 {}，预期为 {expected}",
                        response.api_revision
                    )
                    .into());
                }
            }
            println!("DRAIN_ROUTE_OK={expected}");
        }
        "health" => {
            let mut health = health_pb::health_client::HealthClient::new(channel().await?);
            let response = health
                .check(Request::new(health_pb::HealthCheckRequest {
                    service: "openrust.example.v1.Echo".to_owned(),
                }))
                .await?
                .into_inner();
            let status =
                health_pb::health_check_response::ServingStatus::try_from(response.status)?;
            println!("HEALTH_STATUS={status:?}");
        }
        _ => return Err(format!("未知探针模式：{mode}").into()),
    }
    Ok(())
}
