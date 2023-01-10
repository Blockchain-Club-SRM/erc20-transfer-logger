use std::sync::Arc;

use ethers::providers::{Provider, Ws};

pub struct TrackerClient {
    pub client: Arc<Provider<Ws>>
}

impl TrackerClient {
    pub async fn new(
        wss_url: String
    ) -> Self {
        let provider = Provider::<Ws>::connect(wss_url).await.unwrap();
        let client = Arc::new(provider);
        Self { client }
    }
}