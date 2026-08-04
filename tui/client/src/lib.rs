//! Async HTTP client for the aggregator API
//! (`aggregator/src/malt_radar_aggregator/api.py`). Reached only over
//! Tailscale — see `CLAUDE.md`'s network section — never a public URL.

use malt_radar_core::Fiche;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    pub base_url: String,
    pub api_token: String,
}

impl AggregatorConfig {
    /// Reads `AGGREGATOR_URL` and `AGGREGATOR_API_TOKEN` from the
    /// environment. Populate them via `tui/.env` (see `tui/.env.example`),
    /// mirroring `worker/.env` and `aggregator/.env` — same shared secret
    /// as the worker uses, since `require_token` doesn't distinguish
    /// callers (see `aggregator/src/malt_radar_aggregator/auth.py`).
    pub fn from_env() -> Result<Self, ClientError> {
        let base_url =
            std::env::var("AGGREGATOR_URL").map_err(|_| ClientError::MissingConfig("AGGREGATOR_URL"))?;
        let api_token = std::env::var("AGGREGATOR_API_TOKEN")
            .map_err(|_| ClientError::MissingConfig("AGGREGATOR_API_TOKEN"))?;
        Ok(Self { base_url, api_token })
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("missing required environment variable: {0}")]
    MissingConfig(&'static str),
    #[error("request to aggregator failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("aggregator rejected the token (HTTP 401) — check AGGREGATOR_API_TOKEN")]
    Unauthorized,
}

pub struct AggregatorClient {
    http: reqwest::Client,
    config: AggregatorConfig,
}

impl AggregatorClient {
    pub fn new(config: AggregatorConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    /// `GET /fiches?limit=<limit>` — see `aggregator/README.md`.
    pub async fn list_fiches(&self, limit: u32) -> Result<Vec<Fiche>, ClientError> {
        let url = format!("{}/fiches", self.config.base_url.trim_end_matches('/'));
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.config.api_token)
            .query(&[("limit", limit)])
            .send()
            .await?;

        if response.status().as_u16() == 401 {
            return Err(ClientError::Unauthorized);
        }
        let fiches = response.error_for_status()?.json::<Vec<Fiche>>().await?;
        Ok(fiches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> AggregatorClient {
        AggregatorClient::new(AggregatorConfig {
            base_url: server.uri(),
            api_token: "test-token".to_string(),
        })
    }

    #[tokio::test]
    async fn list_fiches_sends_bearer_token_and_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/fiches"))
            .and(query_param("limit", "50"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"filename": "a.md", "title": "Consultant RAG", "stack": ["Python"]}
            ])))
            .mount(&server)
            .await;

        let fiches = client_for(&server).list_fiches(50).await.unwrap();

        assert_eq!(fiches.len(), 1);
        assert_eq!(fiches[0].title.as_deref(), Some("Consultant RAG"));
        assert_eq!(fiches[0].stack, vec!["Python".to_string()]);
    }

    #[tokio::test]
    async fn list_fiches_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/fiches"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = client_for(&server).list_fiches(50).await;

        assert!(matches!(result, Err(ClientError::Unauthorized)));
    }

    #[tokio::test]
    async fn list_fiches_surfaces_other_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/fiches"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let result = client_for(&server).list_fiches(50).await;

        assert!(matches!(result, Err(ClientError::Request(_))));
    }
}
