//! 具名内容审核驱动以及应用和供应商之间的审核契约。

#![forbid(unsafe_code)]

#[path = "../config/moderation.rs"]
mod configuration;
mod contracts;
mod drivers;
mod input;
mod manager;
mod moderation;
mod provider;
mod result;

pub use contracts::{
    ModerationDriver, ModerationError, ModerationFuture, ModerationResult, Operation,
};
pub use input::{ImageInput, StoredMedia, TaskId, TextInput, VideoInput};
pub use manager::ModerationManager;
pub use moderation::Moderation;
pub use provider::{ModerationServiceProvider, openrust_providers};
pub use result::{Assessment, Confidence, Coverage, Decision, Finding, Modality, TaskStatus};
