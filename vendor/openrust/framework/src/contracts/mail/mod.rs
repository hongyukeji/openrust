//! Mail selection, composition, and queued delivery contracts.

mod factory;
mod mail_queue;
mod mailable;
mod mailer;

pub use factory::Factory;
pub use mail_queue::MailQueue;
pub use mailable::Mailable;
pub use mailer::Mailer;
