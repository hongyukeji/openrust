use std::{process::Stdio, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

use crate::mail::{MailFuture, MailResult, Message, SentMessage, transport};

use super::Transport;

pub(in crate::mail) struct SendmailTransport {
    program: String,
    args: Vec<String>,
    use_smtp: bool,
}

impl SendmailTransport {
    pub(in crate::mail) fn new(command: &str) -> MailResult<Self> {
        let words = shlex::split(command).ok_or_else(|| transport("Invalid sendmail command."))?;
        let (program, args) = words
            .split_first()
            .filter(|(program, _)| !program.is_empty())
            .ok_or_else(|| transport("A sendmail command is required."))?;
        let use_smtp = args.iter().any(|arg| arg == "-bs");
        if use_smtp && args.iter().any(|arg| arg == "-t") {
            return Err(transport("Sendmail modes -bs and -t cannot be combined."));
        }
        Ok(Self {
            program: program.clone(),
            args: args.iter().filter(|arg| *arg != "-t").cloned().collect(),
            use_smtp,
        })
    }

    async fn deliver(&self, message: &Message) -> MailResult<()> {
        let mut process = Command::new(&self.program);
        process
            .args(&self.args)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stderr(Stdio::null());
        let envelope = message.0.envelope();
        let sender = envelope
            .from()
            .ok_or_else(|| transport("A sendmail sender is required."))?;
        if self.use_smtp {
            process.stdout(Stdio::piped());
        } else {
            if !self.args.iter().any(|arg| arg == "-i" || arg == "-oi") {
                process.arg("-i");
            }
            process
                .args(["-f", sender.as_ref(), "--"])
                .args(envelope.to())
                .stdout(Stdio::null());
        }
        let mut child = process
            .spawn()
            .map_err(|_| transport("Could not start sendmail."))?;
        let mut input = child.stdin.take().expect("configured sendmail stdin");
        let mime = message.formatted();
        if let Some(output) = child.stdout.take() {
            let mut output = BufReader::new(output);
            expect_reply(&mut output, &[220]).await?;
            exchange(&mut input, &mut output, b"EHLO localhost\r\n", &[250]).await?;
            exchange(
                &mut input,
                &mut output,
                format!("MAIL FROM:<{sender}>\r\n").as_bytes(),
                &[250],
            )
            .await?;
            for recipient in envelope.to() {
                exchange(
                    &mut input,
                    &mut output,
                    format!("RCPT TO:<{recipient}>\r\n").as_bytes(),
                    &[250, 251],
                )
                .await?;
            }
            exchange(&mut input, &mut output, b"DATA\r\n", &[354]).await?;
            let mut data = Vec::with_capacity(mime.len() + 5);
            for line in mime.split_inclusive(|byte| *byte == b'\n') {
                if line.starts_with(b".") {
                    data.push(b'.');
                }
                data.extend_from_slice(line);
            }
            if !data.ends_with(b"\r\n") {
                data.extend_from_slice(b"\r\n");
            }
            data.extend_from_slice(b".\r\n");
            exchange(&mut input, &mut output, &data, &[250]).await?;
            // The message has already been accepted. QUIT cannot change that outcome.
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                input.write_all(b"QUIT\r\n").await?;
                drop(input);
                child.wait().await
            })
            .await;
            return Ok(());
        }
        let write = input.write_all(&mime).await;
        drop(input);
        let status = child
            .wait()
            .await
            .map_err(|_| transport("Could not wait for sendmail."))?;
        if !status.success() {
            return Err(transport("Sendmail rejected the message."));
        }
        write.map_err(|_| transport("Could not write the message to sendmail."))
    }
}

impl Transport for SendmailTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(60), self.deliver(message))
                .await
                .map_err(|_| transport("Sendmail timed out."))??;
            Ok(SentMessage {
                message: message.clone(),
            })
        })
    }
}

async fn exchange(
    input: &mut (impl AsyncWriteExt + Unpin),
    output: &mut (impl AsyncBufReadExt + Unpin),
    data: &[u8],
    codes: &[u16],
) -> MailResult<()> {
    input
        .write_all(data)
        .await
        .map_err(|_| transport("Could not write to sendmail."))?;
    expect_reply(output, codes).await
}

async fn expect_reply(
    output: &mut (impl AsyncBufReadExt + Unpin),
    codes: &[u16],
) -> MailResult<()> {
    let mut line = String::new();
    loop {
        if output
            .read_line(&mut line)
            .await
            .map_err(|_| transport("Could not read the sendmail reply."))?
            == 0
        {
            return Err(transport("Sendmail closed the SMTP session."));
        }
        let code = line
            .get(..3)
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| transport("Invalid sendmail SMTP reply."))?;
        if !codes.contains(&code) {
            return Err(transport(format!(
                "Sendmail SMTP rejected the request ({code})."
            )));
        }
        if line.as_bytes().get(3) != Some(&b'-') {
            return Ok(());
        }
        line.clear();
    }
}
