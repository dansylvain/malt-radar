//! Async HTTP client for the aggregator API
//! (`aggregator/src/malt_radar_aggregator/api.py`). Reached only over
//! Tailscale — see `CLAUDE.md`'s network section — never a public URL.

use malt_radar_core::{Digest, Fiche, SkillAlias};
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

// `Clone` is needed so `app`'s background tasks (fetch loop, alias-write
// loop) can each hold their own handle — cheap: `reqwest::Client` is
// `Arc`-backed internally, and `AggregatorConfig` already derives `Clone`.
#[derive(Clone)]
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

    /// `DELETE /fiches/{filename}` — unlike `delete_skill_alias`, `filename`
    /// is a path segment, not a query param: it's server-derived
    /// (`_fiche_filename`) and never contains `/`, so path routing is safe.
    pub async fn delete_fiche(&self, filename: &str) -> Result<bool, ClientError> {
        let url = format!("{}/fiches/{}", self.config.base_url.trim_end_matches('/'), filename);
        let response = self.http.delete(url).bearer_auth(&self.config.api_token).send().await?;

        if response.status().as_u16() == 401 {
            return Err(ClientError::Unauthorized);
        }
        let body: serde_json::Value = response.error_for_status()?.json().await?;
        Ok(body.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    /// `GET /skill-aliases` — see `aggregator/README.md`.
    pub async fn list_skill_aliases(&self) -> Result<Vec<SkillAlias>, ClientError> {
        let url = format!("{}/skill-aliases", self.config.base_url.trim_end_matches('/'));
        let response = self.http.get(url).bearer_auth(&self.config.api_token).send().await?;

        if response.status().as_u16() == 401 {
            return Err(ClientError::Unauthorized);
        }
        Ok(response.error_for_status()?.json::<Vec<SkillAlias>>().await?)
    }

    /// `PUT /skill-aliases`.
    pub async fn upsert_skill_alias(&self, alias: &str, canonical: &str) -> Result<SkillAlias, ClientError> {
        let url = format!("{}/skill-aliases", self.config.base_url.trim_end_matches('/'));
        let response = self
            .http
            .put(url)
            .bearer_auth(&self.config.api_token)
            .json(&serde_json::json!({ "alias": alias, "canonical": canonical }))
            .send()
            .await?;

        if response.status().as_u16() == 401 {
            return Err(ClientError::Unauthorized);
        }
        Ok(response.error_for_status()?.json::<SkillAlias>().await?)
    }

    /// `DELETE /skill-aliases` — `alias` is sent as a query param, not a
    /// path segment or body: skill strings can contain "/" (breaks path
    /// routing), and `httpx`'s `.delete()` shorthand (used server-side by
    /// the aggregator's tests) has no body support at all.
    pub async fn delete_skill_alias(&self, alias: &str) -> Result<bool, ClientError> {
        let url = format!("{}/skill-aliases", self.config.base_url.trim_end_matches('/'));
        let response = self
            .http
            .delete(url)
            .bearer_auth(&self.config.api_token)
            .query(&[("alias", alias)])
            .send()
            .await?;

        if response.status().as_u16() == 401 {
            return Err(ClientError::Unauthorized);
        }
        let body: serde_json::Value = response.error_for_status()?.json().await?;
        Ok(body.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    /// `GET /digest` — the latest generated digest. A 404 means the daily
    /// timer hasn't produced one yet (a normal pre-first-run state, not an
    /// error) and maps to `Ok(None)` rather than `ClientError`.
    pub async fn get_digest(&self) -> Result<Option<Digest>, ClientError> {
        let url = format!("{}/digest", self.config.base_url.trim_end_matches('/'));
        let response = self.http.get(url).bearer_auth(&self.config.api_token).send().await?;

        if response.status().as_u16() == 401 {
            return Err(ClientError::Unauthorized);
        }
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        let digest = response.error_for_status()?.json::<Digest>().await?;
        Ok(Some(digest))
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

    #[tokio::test]
    async fn delete_fiche_sends_filename_as_path_segment() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/fiches/free_work_1.md"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"filename": "free_work_1.md", "deleted": true})))
            .mount(&server)
            .await;

        let deleted = client_for(&server).delete_fiche("free_work_1.md").await.unwrap();

        assert!(deleted);
    }

    #[tokio::test]
    async fn delete_fiche_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/fiches/free_work_1.md"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = client_for(&server).delete_fiche("free_work_1.md").await;

        assert!(matches!(result, Err(ClientError::Unauthorized)));
    }

    #[tokio::test]
    async fn list_skill_aliases_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/skill-aliases"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"alias_key": "powerbi", "canonical": "Power BI", "updated_at": "2026-08-04T00:00:00Z"}
            ])))
            .mount(&server)
            .await;

        let aliases = client_for(&server).list_skill_aliases().await.unwrap();

        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].alias_key, "powerbi");
        assert_eq!(aliases[0].canonical, "Power BI");
    }

    #[tokio::test]
    async fn list_skill_aliases_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/skill-aliases"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = client_for(&server).list_skill_aliases().await;

        assert!(matches!(result, Err(ClientError::Unauthorized)));
    }

    #[tokio::test]
    async fn upsert_skill_alias_sends_json_body() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/skill-aliases"))
            .and(header("Authorization", "Bearer test-token"))
            .and(wiremock::matchers::body_json(serde_json::json!({"alias": "PowerBI", "canonical": "Power BI"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(
                {"alias_key": "powerbi", "canonical": "Power BI", "updated_at": "2026-08-04T00:00:00Z"}
            )))
            .mount(&server)
            .await;

        let alias = client_for(&server).upsert_skill_alias("PowerBI", "Power BI").await.unwrap();

        assert_eq!(alias.alias_key, "powerbi");
    }

    #[tokio::test]
    async fn upsert_skill_alias_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/skill-aliases"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = client_for(&server).upsert_skill_alias("rag", "RAG").await;

        assert!(matches!(result, Err(ClientError::Unauthorized)));
    }

    #[tokio::test]
    async fn delete_skill_alias_sends_alias_as_query_param_not_body() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/skill-aliases"))
            .and(query_param("alias", "rag"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"alias_key": "rag", "deleted": true})))
            .mount(&server)
            .await;

        let deleted = client_for(&server).delete_skill_alias("rag").await.unwrap();

        assert!(deleted);
    }

    #[tokio::test]
    async fn delete_skill_alias_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/skill-aliases"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = client_for(&server).delete_skill_alias("rag").await;

        assert!(matches!(result, Err(ClientError::Unauthorized)));
    }

    #[tokio::test]
    async fn get_digest_returns_parsed_digest() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/digest"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 1,
                "created_at": "2026-08-05T08:00:00+00:00",
                "window_start": "2026-07-29T08:00:00+00:00",
                "window_end": "2026-08-05T08:00:00+00:00",
                "window_days": 7,
                "model": "claude-haiku-4-5",
                "fiche_count": 12,
                "metrics": {"fiche_count": 12},
                "trend": {"delta": null},
                "summary_short": "Tout va bien.",
                "content": "## Résumé exécutif\n..."
            })))
            .mount(&server)
            .await;

        let digest = client_for(&server).get_digest().await.unwrap();

        let digest = digest.expect("expected Some(Digest)");
        assert_eq!(digest.id, 1);
        assert_eq!(digest.fiche_count, 12);
        assert_eq!(digest.model, "claude-haiku-4-5");
        assert_eq!(digest.summary_short, "Tout va bien.");
        assert_eq!(digest.content, "## Résumé exécutif\n...");
    }

    #[tokio::test]
    async fn get_digest_maps_404_to_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/digest"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let digest = client_for(&server).get_digest().await.unwrap();

        assert!(digest.is_none());
    }

    #[tokio::test]
    async fn get_digest_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/digest"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = client_for(&server).get_digest().await;

        assert!(matches!(result, Err(ClientError::Unauthorized)));
    }

    #[tokio::test]
    async fn get_digest_surfaces_other_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/digest"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let result = client_for(&server).get_digest().await;

        assert!(matches!(result, Err(ClientError::Request(_))));
    }
}
