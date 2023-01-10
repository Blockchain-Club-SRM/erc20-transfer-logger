use ethers::prelude::Abigen;
use serde_aux::field_attributes::deserialize_number_from_string;
use secrecy::{ExposeSecret, Secret};
use sqlx::{postgres::{PgConnectOptions, PgSslMode}};
use url::Url;

// use crate::{email_client::EmailClient, domain::UserEmail};

pub enum Environment {
    Local,
    Production,
}

impl Environment {
    pub fn as_str(&self) -> &str {
        match self {
            Environment::Local => "local",
            Environment::Production => "production",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;
    fn try_from(value:String)->Result<Self,Self::Error>{
        match value.as_str() {
            "local" => Ok(Environment::Local),
            "production" => Ok(Environment::Production),
            _ => Err(format!("Unknown environment: {}", value)),
        }
    }
}


#[derive(serde::Deserialize,Clone)]
pub struct Settings {
    // pub application: ApplicationSetting,
    // pub email_client: EmailClientSetting,
    pub tracker: TrackerSetting,
    pub database: DatabaseSetting,
}

// #[derive(serde::Deserialize,Clone)]
// pub struct ApplicationSetting {
//     pub host: String,
//     pub port: u16,
//     pub base_url: String,
// }

// impl ApplicationSetting {
//     pub fn url(&self) -> String {
//         format!("{}:{}", self.host, self.port)
//     }
// }

// #[derive(serde::Deserialize,Clone)]
// pub struct EmailClientSetting {
//     pub base_url: String,
//     pub sender_email: String,
//     pub authorization_token: Secret<String>,
//     #[serde(deserialize_with = "deserialize_number_from_string")]
//     pub timeout_milliseconds: u64,
// }

// impl EmailClientSetting {
//     pub fn client(self)-> EmailClient {
//         let timeout = self.timeout();
//         let sender_email = self.sender().expect("Invalid sender email");
//         EmailClient::new(
//             self.base_url,
//             sender_email,
//             self.authorization_token,
//             timeout
//         )
//     }
//     pub fn timeout(&self) -> std::time::Duration {
//         std::time::Duration::from_millis(self.timeout_milliseconds)
//     }
//     pub fn sender(&self) -> Result<UserEmail, String> {
//         UserEmail::parse(self.sender_email.clone())
//     }
// }

#[derive(serde::Deserialize,Clone)]
pub struct TrackerSetting{
    pub url: String,
    pub abi_file_location: String,
}

impl TrackerSetting {
    pub fn is_valid_websocket_url(url: &str) -> bool {
        match Url::parse(url) {
            Ok(parsed_url) => parsed_url.scheme() == "ws" || parsed_url.scheme() == "wss",
            Err(_) => false,
        }
    }
    pub fn generate_abi(&self) -> Result<(), String> {
        Abigen::new("IERC20", format!("{}/IERC20.json",self.abi_file_location)).expect("Failed to find file location")
        .generate().expect("Failed to generate")
        .write_to_file(format!("{}/ierc20.rs",self.abi_file_location)).expect("Failed to build");
        Ok(())
    }
}

#[derive(serde::Deserialize,Clone)]
pub struct DatabaseSetting {
    pub host: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub username: String,
    pub password: Secret<String>,
    pub database_name: String,
    pub require_ssl: bool,
}

impl DatabaseSetting {
    pub fn without_db(&self) -> PgConnectOptions {
        let ssl_mode = if self.require_ssl {
            PgSslMode::Require
        } else {
            PgSslMode::Prefer
        };
        PgConnectOptions::new()
            .host(&self.host)
            .username(&self.username)
            .password(self.password.expose_secret())
            .port(self.port)
            .ssl_mode(ssl_mode)
    }

    pub fn with_db(&self) -> PgConnectOptions {
        self.without_db().database(&self.database_name)
    }
}

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let base_path= std::env::current_dir().expect("Failed to get current directory");
    let configuration_directory = base_path.join("configuration");
    let environment:Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "local".into())
        .try_into()
        .expect("Unknown environment");
        let environment_filename = format!("{}.yaml", environment.as_str());
        let settings = config::Config::builder()
        .add_source(config::File::from(
            configuration_directory.join("base.yaml")
    ))
    .add_source(config::File::from(
        configuration_directory.join(environment_filename)
    ))
    .add_source(
        config::Environment::with_prefix("APP")
            .prefix_separator("_")
            .separator("__"),
    )
    .build()?;
    settings.try_deserialize::<Settings>()

}