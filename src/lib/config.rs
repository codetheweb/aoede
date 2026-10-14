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
    #[serde(default)]
    pub spotify_username: String,
    #[serde(alias = "SPOTIFY_PASSWORD")]
    #[serde(default)]
    pub spotify_password: String,
    #[serde(alias = "DISCORD_USER_ID")]
    pub discord_user_id: u64,
    #[serde(alias = "SPOTIFY_BOT_AUTOPLAY")]
    #[serde(default)]
    pub spotify_bot_autoplay: bool,
    #[serde(alias = "SPOTIFY_DEVICE_NAME")]
    #[serde(default = "default_spotify_device_name")]
    pub spotify_device_name: String,
    #[serde(alias = "CACHE_DIR")]
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
}

fn default_spotify_device_name() -> String {
    "Aoede".to_string()
}

fn default_cache_dir() -> String {
    "aoede-cache".to_string()
}

impl Config {
    pub fn new() -> Result<Self, Box<Error>> {
        let config: Config = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::raw())
            .extract()?;
        Ok(config)
    }
}
