use std::fs::read_to_string;
use serde::{Deserialize, Deserializer};
use serenity::model::id;

#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(rename = "DISCORD_TOKEN")]
    pub discord_token: String,
    #[serde(rename = "SPOTIFY_USERNAME")]
    pub spotify_username: String,
    #[serde(rename = "SPOTIFY_PASSWORD")]
    pub spotify_password: String,
    #[serde(rename = "DISCORD_USER_ID", deserialize_with = "discord_id_from_string")]
    pub discord_user_id: id::UserId,
}

fn discord_id_from_string<'de, D>(deserializer: D) -> Result<id::UserId, D::Error> where D: Deserializer<'de>, {
  let s: &str = Deserialize::deserialize(deserializer)?;

  Ok(id::UserId::from(s.parse::<u64>().unwrap()))
}

#[derive(Clone)]
pub struct ConfigWrapper {
  pub config: Config
}

impl ConfigWrapper {
  pub fn new() -> ConfigWrapper {
    match envy::from_env::<Config>() {
      Ok(config) => ConfigWrapper {
        config
      },
      Err(err) => {
        panic!("%{}", err);
        let config_string = read_to_string("config.toml").expect("environment variables are missing and config.toml could not be read");

        let config: Config = toml::from_str(&config_string).expect("config.toml is incorrect");

        ConfigWrapper {
          config
        }
      }
    }
  }
}
