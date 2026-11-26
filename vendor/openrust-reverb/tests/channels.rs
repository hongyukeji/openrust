use openrust_reverb::protocols::pusher::channels::{ChannelBroker, Frame, Member};
use serde_json::{Value, json};
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;

fn connect(
    broker: &ChannelBroker,
    id: &str,
    capacity: usize,
) -> (Receiver<Frame>, CancellationToken) {
    let (sender, receiver) = mpsc::channel(capacity);
    let cancel = CancellationToken::new();
    assert!(broker.connect(id.into(), sender, cancel.clone(), None));
    (receiver, cancel)
}

fn frame(receiver: &mut Receiver<Frame>, event: &str) -> Value {
    let frame = receiver.try_recv().unwrap();
    assert_eq!(frame.event, event);
    serde_json::from_str(&frame.data).unwrap()
}

#[test]
fn presence_counts_distinct_members_and_announces_only_first_join_and_last_leave() {
    let broker = ChannelBroker::default();
    let channel = "presence-room";
    let (mut first, _) = connect(&broker, "1.1", 8);
    let (mut second, _) = connect(&broker, "1.2", 8);
    let (mut extra, _) = connect(&broker, "1.3", 8);
    let alice = Member {
        user_id: "alice".into(),
        user_info: json!({"name":"Alice"}),
    };
    let bob = Member {
        user_id: "bob".into(),
        user_info: json!({"name":"Bob"}),
    };
    broker
        .subscribe("1.1", channel, Some(alice.clone()))
        .unwrap();
    assert_eq!(
        frame(&mut first, "pusher_internal:subscription_succeeded")["presence"]["ids"],
        json!(["alice"])
    );
    broker.subscribe("1.2", channel, Some(bob)).unwrap();
    assert_eq!(
        frame(&mut second, "pusher_internal:subscription_succeeded")["presence"]["count"],
        2
    );
    assert_eq!(
        frame(&mut first, "pusher_internal:member_added")["user_id"],
        "bob"
    );
    broker.subscribe("1.3", channel, Some(alice)).unwrap();
    assert_eq!(
        frame(&mut extra, "pusher_internal:subscription_succeeded")["presence"]["count"],
        2
    );
    assert!(first.try_recv().is_err() && second.try_recv().is_err());

    broker.disconnect("1.1");
    assert!(second.try_recv().is_err());
    broker.unsubscribe("1.3", channel);
    assert_eq!(
        frame(&mut second, "pusher_internal:member_removed")["user_id"],
        "alice"
    );
}

#[test]
fn publish_excludes_the_origin_and_disconnects_slow_subscribers() {
    let broker = ChannelBroker::default();
    let (mut first, first_cancel) = connect(&broker, "1.1", 4);
    let (mut second, second_cancel) = connect(&broker, "1.2", 1);
    for socket in ["1.1", "1.2"] {
        broker.subscribe(socket, "news", None).unwrap();
    }
    frame(&mut first, "pusher_internal:subscription_succeeded");
    frame(&mut second, "pusher_internal:subscription_succeeded");

    broker.publish("news", "example", "{}".into(), Some("1.1"));
    assert!(first.try_recv().is_err());
    assert!(!second_cancel.is_cancelled());
    broker.publish("news", "example", "{}".into(), Some("1.1"));
    assert!(second_cancel.is_cancelled());
    assert!(!first_cancel.is_cancelled());
    frame(&mut second, "example");
}
