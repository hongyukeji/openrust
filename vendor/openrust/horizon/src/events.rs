use openrust::{
    contracts::events::Event,
    queue::{Payload, events::JobSnapshot},
};
use std::time::Duration;

macro_rules! enqueue_event {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        pub struct $name {
            pub connection_name: String,
            pub queue: String,
            pub id: String,
            pub payload: Payload,
        }

        impl $name {
            pub const NAME: &str = stringify!($name);
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}

enqueue_event!(
    /// Redis 写入任务前的投递快照。
    JobPending
);

enqueue_event!(
    /// Redis 接受即时或延迟任务后的投递快照。
    JobPushed
);

/// Redis 保留任务后、Worker 执行前的快照。队列、投递 ID、载荷和尝试次数在 `job` 中。
#[derive(Clone, Debug)]
pub struct JobReserved {
    pub connection_name: String,
    pub job: JobSnapshot,
}

impl JobReserved {
    pub const NAME: &str = "JobReserved";
}

impl Event for JobReserved {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// 一批从延迟或过期保留集合迁回 Redis 就绪队列的任务。
#[derive(Clone, Debug)]
pub struct JobsMigrated {
    pub connection_name: String,
    pub queue: String,
    pub payloads: Vec<JobSnapshot>,
}

impl JobsMigrated {
    pub const NAME: &str = "JobsMigrated";
}

impl Event for JobsMigrated {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// 已确认重新入队的 Redis 保留任务及其延迟。
#[derive(Clone, Debug)]
pub struct JobReleased {
    pub connection_name: String,
    pub job: JobSnapshot,
    pub delay: Duration,
}

impl JobReleased {
    pub const NAME: &str = "Horizon.JobReleased";
}

impl Event for JobReleased {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// 已确认删除的 Redis 保留任务、失败标记和保留后的实际时长。
#[derive(Clone, Debug)]
pub struct JobDeleted {
    pub connection_name: String,
    pub job: JobSnapshot,
    pub failed: bool,
    pub duration: Duration,
}

impl JobDeleted {
    pub const NAME: &str = "JobDeleted";
}

impl Event for JobDeleted {
    fn name(&self) -> &str {
        Self::NAME
    }
}
