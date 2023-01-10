use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{configuration::{
    // Settings, 
    DatabaseSetting}, 
    // tracker_client::TrackerClient
};

// pub struct Application {
//     pool: PgPool,
//     client: TrackerClient
// }

// impl Application {
//     pub async fn build(configuration: Settings) -> Result<Self, std::io::Error> {
//         let connection_pool = get_connection_pool(&configuration.database);
//         let tracker_client = TrackerClient::new(configuration.tracker.url);
//         Ok(Self {
//             pool: connection_pool,
//             client: tracker_client.await
//         })
//     }
// }



pub fn get_connection_pool(configuration: &DatabaseSetting) -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy_with(configuration.with_db())
}