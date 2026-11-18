//! Mail selection, composition, and queued delivery contracts.

mod attachable;
mod factory;
mod mail_queue;
mod mailable;
mod mailer;

pub use attachable::Attachable;
pub use factory::Factory;
pub use mail_queue::MailQueue;
pub use mailable::Mailable;
pub use mailer::Mailer;
