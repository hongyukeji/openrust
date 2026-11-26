use crate::support::MessageBag;

/// 对外提供已有消息包；读取时不重新执行验证。
pub trait MessageProvider {
    fn get_message_bag(&self) -> &MessageBag;
}
