use reqwest::Url;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: Url,
}

impl ClientConfig {
    pub fn new(base_url: Url) -> Self {
        Self { base_url }
    }
}
