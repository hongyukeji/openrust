use openrust::{
    contracts::events::{Event, Listener},
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
    notifications::NotificationEvent,
};

/// 关闭套件验证时，沿用通知发送事件阻止默认User仍具备的验证邮件能力。
/// 不影响密码找回或应用自身的其他通知，也不替换全局注册事件监听器。
pub(super) struct DisableVerificationMail;
impl Listener for DisableVerificationMail {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            Ok(event
                .downcast_ref::<NotificationEvent>()
                .filter(|event| event.kind == "VerifyEmail" && event.channel == "mail")
                .map(|_| ListenerResponse::Stop))
        })
    }
}
