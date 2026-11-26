use crate::{Error, MasterSupervisor, Result, Supervisor};
use openrust::{contracts::container::Container as _, support::facades::App};
use serde_json::Value;
use std::{future::Future, pin::Pin, sync::Arc};

pub type CommandFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// 将实现以线协议命令名绑定到容器，处理 Master 领取的结构化命令。
pub trait MasterSupervisorCommandHandler: Send + Sync {
    fn process<'a>(
        &'a self,
        master: &'a mut MasterSupervisor,
        options: Value,
    ) -> CommandFuture<'a, ()>;
}

/// 将实现以线协议命令名绑定到容器，处理 Supervisor 领取的结构化命令。
pub trait SupervisorCommandHandler: Send + Sync {
    /// 返回退出码时，Supervisor 进入正常的排空和退出流程。
    fn process<'a>(
        &'a self,
        supervisor: &'a mut Supervisor,
        options: Value,
    ) -> CommandFuture<'a, Option<u8>>;
}

pub(crate) fn resolve<T: ?Sized + Send + Sync + 'static>(name: &str) -> Result<Option<Arc<T>>> {
    let Ok(container) = App::get_container() else {
        return Ok(None);
    };
    if container.has(name) {
        return container
            .get::<T>(name)
            .map(Some)
            .map_err(|_| Error::InvalidConfiguration);
    }
    Ok(None)
}
