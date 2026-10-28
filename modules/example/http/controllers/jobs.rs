//! 通过公开 Queue 服务派发模块任务。

use openrust::prelude::*;

pub(crate) async fn enqueue_example_heartbeat(
    Inject(queue): Inject<openrust::queue::Queue>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, String>>> {
    use openrust::queue::Job;
    let id = queue
        .dispatch(
            "example",
            <crate::modules::example::jobs::example_heartbeat::ExampleHeartbeatJob as Job>::NAME,
            &crate::modules::example::jobs::example_heartbeat::ExampleHeartbeatJob,
        )
        .await?;
    Ok(Json(std::collections::BTreeMap::from([("job_id", id)])))
}
