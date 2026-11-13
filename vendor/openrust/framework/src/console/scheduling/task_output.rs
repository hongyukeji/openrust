use std::{fs::File, path::PathBuf};

use crate::{
    console::ConsoleError, contracts::foundation::Application as _, foundation::Application,
};

use super::error;

pub(super) struct Destination {
    pub path: PathBuf,
    pub append: bool,
}

impl Destination {
    pub fn open(&self, app: &Application) -> Result<File, ConsoleError> {
        let path = app.base_path().join(&self.path);
        File::options()
            .create(true)
            .write(true)
            .append(self.append)
            .truncate(!self.append)
            .open(&path)
            .map_err(|source| {
                error(format!(
                    "Cannot open scheduled output {}: {source}",
                    path.display()
                ))
            })
    }
}

pub(super) async fn email(
    recipients: &[crate::mail::mailables::Address],
    subject: String,
    output: &[u8],
) -> Result<(), ConsoleError> {
    use crate::mail::{
        MailResult,
        mailables::{Content, Envelope},
    };

    struct ScheduledOutput(Envelope, Content);
    impl crate::contracts::mail::Mailable for ScheduledOutput {
        fn envelope(&self) -> MailResult<Envelope> {
            Ok(self.0.clone())
        }

        fn content(&self) -> MailResult<Content> {
            Ok(self.1.clone())
        }
    }

    let text = if output.is_empty() {
        "No output".to_owned()
    } else {
        String::from_utf8_lossy(output).into_owned()
    };
    crate::support::facades::Mail::sendNow(&ScheduledOutput(
        Envelope {
            to: recipients.to_vec(),
            subject,
            ..Envelope::default()
        },
        Content {
            text: Some(text),
            ..Content::default()
        },
    ))
    .await
    .map_err(error)?;
    Ok(())
}
