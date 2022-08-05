use figment::{
    providers::{Env, Format, Toml},
    Error, Figment,
};
use serde::Deserialize;
use serenity::model::id;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Config {
    pub discord_token: String,
    pub spotify_username: String,
    pub spotify_password: String,
    pub discord_user_id: id::UserId,
}

impl Config {
    pub fn new() -> Result<Self, Error> {
        let config: Config = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::raw().map(|v| v.to_string().to_lowercase().into()))
            .extract()?;
        Ok(config)
    }
}
