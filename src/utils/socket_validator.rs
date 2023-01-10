use url::Url;

pub fn is_valid_websocket_url(url: &str) -> bool {
    match Url::parse(url) {
        Ok(parsed_url) => parsed_url.scheme() == "ws" || parsed_url.scheme() == "wss",
        Err(_) => false,
    }
}

