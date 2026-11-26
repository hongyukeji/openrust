use openrust::validation::{Rule, rules::Password};

pub(super) fn password_rules() -> Vec<Rule> {
    vec![
        Rule::required(),
        Rule::string(),
        Password::default().into(),
        Rule::confirmed(),
    ]
}
