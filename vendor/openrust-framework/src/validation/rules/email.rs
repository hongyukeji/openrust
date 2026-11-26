mod dns;
mod native;
mod rfc;
mod spoof;
use crate::validation::{Rule, rule::Kind};
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Mode {
    Rfc,
    Strict,
    Dns,
    Spoof,
    Filter,
    FilterUnicode,
}
#[derive(Clone, Default)]
pub struct Email {
    modes: Vec<Mode>,
    pub(in crate::validation) custom_rules: Vec<Rule>,
}
impl Email {
    pub fn new() -> Self {
        Self::default()
    }
    fn mode(mut self, mode: Mode) -> Self {
        if !self.modes.contains(&mode) {
            self.modes.push(mode);
            self.modes.sort_unstable();
        }
        self
    }
    pub fn rfc_compliant(self, strict: bool) -> Self {
        self.mode(if strict { Mode::Strict } else { Mode::Rfc })
    }
    pub fn strict(self) -> Self {
        self.rfc_compliant(true)
    }
    pub fn validate_mx_record(self) -> Self {
        self.mode(Mode::Dns)
    }
    pub fn prevent_spoofing(self) -> Self {
        self.mode(Mode::Spoof)
    }
    pub fn with_native_validation(self, allow_unicode: bool) -> Self {
        self.mode(if allow_unicode {
            Mode::FilterUnicode
        } else {
            Mode::Filter
        })
    }
    pub fn rules(mut self, rules: impl IntoIterator<Item = Rule>) -> Self {
        self.custom_rules.extend(rules);
        self
    }
    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }
    pub fn unless(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, callback)
    }
    pub fn when_else(
        self,
        condition: bool,
        yes: impl FnOnce(Self) -> Self,
        no: impl FnOnce(Self) -> Self,
    ) -> Self {
        if condition { yes(self) } else { no(self) }
    }
    pub(in crate::validation) fn parse(parameters: &str) -> Self {
        crate::validation::rule_parameters::csv(parameters)
            .into_iter()
            .fold(Self::new(), |rule, mode| {
                rule.mode(match mode.as_ref() {
                    "strict" => Mode::Strict,
                    "dns" => Mode::Dns,
                    "spoof" => Mode::Spoof,
                    "filter" => Mode::Filter,
                    "filter_unicode" => Mode::FilterUnicode,
                    _ => Mode::Rfc,
                })
            })
    }
    pub(in crate::validation) fn parameters(&self) -> Vec<String> {
        self.modes
            .iter()
            .map(|mode| {
                match mode {
                    Mode::Rfc => "rfc",
                    Mode::Strict => "strict",
                    Mode::Dns => "dns",
                    Mode::Spoof => "spoof",
                    Mode::Filter => "filter",
                    Mode::FilterUnicode => "filter_unicode",
                }
                .to_owned()
            })
            .collect()
    }
    pub(in crate::validation) async fn passes(
        &self,
        value: &str,
        fake_dns: bool,
        resolver: Option<&hickory_resolver::TokioResolver>,
    ) -> bool {
        if value.contains(['\r', '\n']) {
            return false;
        }
        let modes = if self.modes.is_empty() {
            &[Mode::Rfc][..]
        } else {
            self.modes.as_slice()
        };
        let mut passes = true;
        for mode in modes {
            let valid = match mode {
                Mode::Rfc => rfc::passes(value),
                Mode::Strict => rfc::strict(value),
                Mode::Filter => native::passes(value, false),
                Mode::FilterUnicode => native::passes(value, true),
                Mode::Spoof => spoof::passes(value),
                Mode::Dns => dns::passes(value, fake_dns, resolver).await,
            };
            passes &= valid;
        }
        passes
    }
}
impl From<Email> for Rule {
    fn from(rule: Email) -> Self {
        Self::new(Kind::Email(rule))
    }
}
