# openrust-mail

Mail layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M3). Planned surface: `Email` trait, `Mailer` service with `send` and `queue`, drivers `SmtpMailer` (lettre, feature `mail-smtp`), `LogMailer` and `FakeMailer` with assertions.

**Status:** scaffold only. Scope and contracts are defined in the framework design; implementation lands in its milestone.
