mod abstract_provider;
mod github_provider;
mod user;
pub use abstract_provider::{
    AbstractProvider, ProviderConfig, ProviderDefinition, ProviderFuture, ProviderOptions,
};
pub use github_provider::GithubProvider;
pub use user::{Token, User};

mod bitbucket_provider;
mod endpoints;
mod gitlab_provider;
mod linkedin_provider;
mod slack_provider;
pub use bitbucket_provider::BitbucketProvider;
pub use endpoints::ProviderEndpoints;
pub use gitlab_provider::GitlabProvider;
pub use linkedin_provider::LinkedInProvider;
mod linkedin_open_id_provider;
pub use linkedin_open_id_provider::LinkedInOpenIdProvider;
pub use slack_provider::SlackProvider;
mod slack_open_id_provider;
pub use slack_open_id_provider::SlackOpenIdProvider;

mod twitter_provider;
mod x_provider;
pub use twitter_provider::TwitterProvider;
pub use x_provider::XProvider;

mod google_provider;
mod identity_token;
pub use google_provider::GoogleProvider;

mod facebook_provider;
pub use facebook_provider::FacebookProvider;

mod qq_provider;
mod weixin_provider;
mod weixin_web_provider;
pub use qq_provider::QqProvider;
pub use weixin_provider::WeixinProvider;
pub use weixin_web_provider::WeixinWebProvider;
