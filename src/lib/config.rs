use figment::{
    providers::{Env, Format, Toml},
    Error, Figment,
};
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(alias = "DISCORD_TOKEN")]
    pub discord_token: String,
    #[serde(alias = "SPOTIFY_USERNAME")]
    pub spotify_username: String,
    #[serde(alias = "SPOTIFY_PASSWORD")]
    pub spotify_password: String,
    #[serde(alias = "DISCORD_USER_ID")]
    pub discord_user_id: u64,
    #[serde(alias = "GUILD_ID")]
    pub guild_id: u64,
}

impl Config {
    pub fn new() -> Result<Self, Error> {
        let config: Config = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::raw())
            .extract()?;
        Ok(config)
    }
}
