use figment::{
    providers::{Env, Format, Toml},
    Error, Figment,
};
use serde::Deserialize;
use serenity::model::id;

#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(rename = "DISCORD_TOKEN")]
    pub discord_token: String,
    #[serde(rename = "SPOTIFY_USERNAME")]
    pub spotify_username: String,
    #[serde(rename = "SPOTIFY_PASSWORD")]
    pub spotify_password: String,
    #[serde(rename = "DISCORD_USER_ID")]
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
