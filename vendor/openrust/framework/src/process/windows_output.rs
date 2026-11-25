use futures_util::FutureExt as _;
use std::{fs::OpenOptions, io, process::Stdio};
use tokio::{
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    process::Command,
};

pub(super) fn attach(command: &mut Command) -> io::Result<(NamedPipeServer, NamedPipeServer)> {
    let (stdout, stdout_writer) = pipe()?;
    let (stderr, stderr_writer) = pipe()?;
    command.stdout(stdout_writer).stderr(stderr_writer);
    Ok((stdout, stderr))
}

fn pipe() -> io::Result<(NamedPipeServer, Stdio)> {
    // Tokio 的异步读取不占用等待子进程写入的阻塞线程，父进程可独立退出。
    let name = format!(r"\\.\pipe\openrust-stream-{}", uuid::Uuid::now_v7());
    let reader = ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .reject_remote_clients(true)
        .access_outbound(false)
        .create(&name)?;
    let writer = OpenOptions::new().write(true).open(name)?;
    // 写端已连接，ConnectNamedPipe 会立即报告连接状态。
    reader
        .connect()
        .now_or_never()
        .ok_or_else(|| io::Error::other("子进程输出管道连接未就绪"))??;
    Ok((reader, Stdio::from(writer)))
}
