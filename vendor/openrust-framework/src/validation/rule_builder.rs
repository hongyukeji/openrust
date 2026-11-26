use super::{Rule, rule::Kind};

/// 保留既有 Rule::min/Rule::max 构造器，同时允许原生方法链。
pub trait RuleBuilder: Sized {
    fn and(self, rule: Rule) -> Rule;
    fn rfc_compliant(self, strict: bool) -> Rule {
        self.and(super::rules::Email::new().rfc_compliant(strict).into())
    }
    fn validate_mx_record(self) -> Rule {
        self.and(super::rules::Email::new().validate_mx_record().into())
    }
    fn prevent_spoofing(self) -> Rule {
        self.and(super::rules::Email::new().prevent_spoofing().into())
    }
    fn with_native_validation(self, allow_unicode: bool) -> Rule {
        self.and(
            super::rules::Email::new()
                .with_native_validation(allow_unicode)
                .into(),
        )
    }
    fn rules(self, rules: impl IntoIterator<Item = Rule>) -> Rule {
        self.and(Rule::new(Kind::Chained(rules.into_iter().collect())))
    }

    fn custom_field(self, field: &str) -> Rule {
        self.and(super::rules::Confirmed::new().custom_field(field).into())
    }
    fn guard(self, guard: &str) -> Rule {
        self.and(super::rules::CurrentPassword::new().guard(guard).into())
    }
    fn min(self, value: impl Into<bigdecimal::BigDecimal>) -> Rule {
        self.and(Rule::min(value))
    }
    fn max(self, value: impl Into<bigdecimal::BigDecimal>) -> Rule {
        self.and(Rule::max(value))
    }
    fn between(
        self,
        min: impl Into<bigdecimal::BigDecimal>,
        max: impl Into<bigdecimal::BigDecimal>,
    ) -> Rule {
        self.and(Rule::new(Kind::Between(min.into(), max.into())))
    }
    fn exactly(self, value: impl Into<bigdecimal::BigDecimal>) -> Rule {
        self.and(Rule::new(Kind::Size(value.into())))
    }
    fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }
    fn unless(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, callback)
    }
    fn when_else(
        self,
        condition: bool,
        yes: impl FnOnce(Self) -> Self,
        no: impl FnOnce(Self) -> Self,
    ) -> Self {
        if condition { yes(self) } else { no(self) }
    }
    fn ascii(self) -> Rule {
        self.and(super::rules::StringRule::new().ascii().into_constraints())
    }
    fn lowercase(self) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .lowercase()
                .into_constraints(),
        )
    }
    fn uppercase(self) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .uppercase()
                .into_constraints(),
        )
    }
    fn alpha(self, ascii: bool) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .alpha(ascii)
                .into_constraints(),
        )
    }
    fn alpha_dash(self, ascii: bool) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .alpha_dash(ascii)
                .into_constraints(),
        )
    }
    fn alpha_numeric(self, ascii: bool) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .alpha_numeric(ascii)
                .into_constraints(),
        )
    }
    fn starts_with(self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .starts_with(values)
                .into_constraints(),
        )
    }
    fn ends_with(self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .ends_with(values)
                .into_constraints(),
        )
    }
    fn doesnt_start_with(self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .doesnt_start_with(values)
                .into(),
        )
    }
    fn doesnt_end_with(self, values: impl IntoIterator<Item = impl AsRef<str>>) -> Rule {
        self.and(
            super::rules::StringRule::new()
                .doesnt_end_with(values)
                .into(),
        )
    }
    fn keys(self, keys: impl IntoIterator<Item = impl AsRef<str>>) -> Rule {
        self.and(super::rules::ArrayRule::new(keys).into())
    }
}
impl RuleBuilder for Rule {
    fn rfc_compliant(self, strict: bool) -> Rule {
        self.map_email(|rule| rule.rfc_compliant(strict))
    }
    fn validate_mx_record(self) -> Rule {
        self.map_email(|rule| rule.validate_mx_record())
    }
    fn prevent_spoofing(self) -> Rule {
        self.map_email(|rule| rule.prevent_spoofing())
    }
    fn with_native_validation(self, allow_unicode: bool) -> Rule {
        self.map_email(|rule| rule.with_native_validation(allow_unicode))
    }
    fn rules(self, rules: impl IntoIterator<Item = Rule>) -> Rule {
        if matches!(self.0, Kind::Email(_)) {
            self.map_email(|rule| rule.rules(rules))
        } else {
            rules.into_iter().fold(self, |rule, next| rule.and(next))
        }
    }

    fn custom_field(self, field: &str) -> Rule {
        if matches!(self.0, Kind::Confirmed(_)) {
            Rule::new(Kind::Confirmed(Some(field.to_owned())))
        } else {
            self.and(super::rules::Confirmed::new().custom_field(field).into())
        }
    }
    fn guard(self, guard: &str) -> Rule {
        if matches!(self.0, Kind::CurrentPassword(_)) {
            Rule::new(Kind::CurrentPassword(Some(guard.to_owned())))
        } else {
            self.and(super::rules::CurrentPassword::new().guard(guard).into())
        }
    }

    fn and(self, rule: Rule) -> Rule {
        let mut rules = match self.0 {
            Kind::Chained(rules) => rules,
            _ => vec![self],
        };
        rules.push(rule);
        Rule::new(Kind::Chained(rules))
    }
}
