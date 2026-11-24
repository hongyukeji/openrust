use hickory_resolver::{
    TokioResolver,
    config::{NameServerConfigGroup, ResolverConfig},
    name_server::TokioConnectionProvider,
    proto::{
        op::{Message, MessageType, ResponseCode},
        rr::{
            RData, Record, RecordType,
            rdata::{A, AAAA},
        },
    },
};
use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, Rule, RuleSet, Validator},
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn validator(url: &str, resolver: TokioResolver) -> Validator {
    let mut validator = Factory::default()
        .make(
            Data::new(json!({"value":url})).unwrap(),
            RuleSet::new().field("value", [Rule::active_url()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator.set_dns_resolver(resolver);
    validator
}

#[tokio::test]
async fn active_url_queries_real_a_aaaa_and_handles_dns_failures_and_instance_isolation() {
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let failing = Arc::new(AtomicBool::new(false));
    let server_fail = failing.clone();
    let server = tokio::spawn(async move {
        let mut bytes = [0; 4096];
        loop {
            let (size, peer) = socket.recv_from(&mut bytes).await.unwrap();
            let request = Message::from_vec(&bytes[..size]).unwrap();
            let query = request.queries()[0].clone();
            let name = query.name().to_ascii().to_ascii_lowercase();
            if name == "timeout.test." {
                continue;
            }
            let mut response = Message::new();
            response
                .set_id(request.id())
                .set_message_type(MessageType::Response)
                .set_recursion_desired(true)
                .set_recursion_available(true)
                .add_query(query.clone());
            if server_fail.load(Ordering::SeqCst) {
                response.set_response_code(ResponseCode::ServFail);
            } else {
                let record = match (name.as_str(), query.query_type()) {
                    ("a.test." | "recover.test.", RecordType::A) => {
                        Some(RData::A(A("192.0.2.1".parse().unwrap())))
                    }
                    ("aaaa.test.", RecordType::AAAA) => {
                        Some(RData::AAAA(AAAA("2001:db8::1".parse().unwrap())))
                    }
                    _ => None,
                };
                if let Some(data) = record {
                    response.add_answer(Record::from_rdata(query.name().clone(), 0, data));
                } else if !matches!(name.as_str(), "a.test." | "aaaa.test." | "recover.test.") {
                    response.set_response_code(ResponseCode::NXDomain);
                }
            }
            socket
                .send_to(&response.to_vec().unwrap(), peer)
                .await
                .unwrap();
        }
    });
    let config = ResolverConfig::from_parts(
        None,
        Vec::new(),
        NameServerConfigGroup::from_ips_clear(&[address.ip()], address.port(), true),
    );
    let mut builder =
        TokioResolver::builder_with_config(config, TokioConnectionProvider::default());
    builder.options_mut().timeout = Duration::from_millis(100);
    builder.options_mut().attempts = 1;
    builder.options_mut().cache_size = 0;
    let resolver = builder.build();
    for (url, passes) in [
        ("https://a.test/path", true),
        ("https://aaaa.test", true),
        ("https://missing.test", false),
        ("http://localhost", false),
        ("http://127.0.0.1", false),
        ("http://[::1]", false),
        ("http://timeout.test", false),
    ] {
        let mut validator = validator(url, resolver.clone());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), validator.validate())
                .await
                .unwrap()
                .is_ok(),
            passes,
            "{url}"
        );
    }
    failing.store(true, Ordering::SeqCst);
    assert!(
        validator("https://recover.test", resolver.clone())
            .validate()
            .await
            .is_err()
    );
    failing.store(false, Ordering::SeqCst);
    assert!(
        validator("https://recover.test", resolver.clone())
            .validate()
            .await
            .is_ok()
    );
    let mut fake = validator("https://missing.test", resolver.clone());
    fake.fake_dns_lookups(true);
    assert!(fake.validate().await.is_ok());
    assert!(
        validator("https://missing.test", resolver.clone())
            .validate()
            .await
            .is_err()
    );
    fake.fake_dns_lookups(false);
    assert!(fake.validate().await.is_err());
    let mut cancelled = validator("https://timeout.test", resolver.clone());
    assert!(
        tokio::time::timeout(Duration::from_millis(1), cancelled.validate())
            .await
            .is_err()
    );
    assert!(
        validator("https://a.test", resolver)
            .validate()
            .await
            .is_ok()
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
