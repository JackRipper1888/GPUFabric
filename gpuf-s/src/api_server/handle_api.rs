use super::*;

use anyhow::Result;
use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post},
    Router,
};

use crate::api_server::{
    apk, banking_admin, benchmark_evidence, client, compute_map, models, points, pre_evaluation,
    technical_snapshot,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

#[allow(dead_code)] // API server utility methods
impl ApiServer {
    pub async fn run_api_server(self: Arc<Self>, bind_addr: &str, port: u16) -> Result<()> {
        pre_evaluation::start_evidence_retention_worker(self.db_pool.clone());
        let app = self.create_api_router().await;
        let bind = format!("{bind_addr}:{port}");
        if matches!(bind_addr, "0.0.0.0" | "::") {
            warn!(
                "API server is listening on a public address ({}); use a reverse proxy, firewall, and token controls",
                bind_addr
            );
        }
        let listener = tokio::net::TcpListener::bind(&bind).await?;

        info!("API server listening on {}", bind);

        axum::serve(listener, app).await.map_err(Into::into)
    }

    // Create API Router
    pub async fn create_api_router(self: Arc<Self>) -> Router {
        let state = Arc::clone(&self);
        let pre_evaluation_routes = Router::new()
            .route(
                "/internal/v1/banking/device-candidates",
                get(client::get_banking_device_candidates),
            )
            .route(
                "/api/banking/provider/benchmark-evidence",
                post(benchmark_evidence::register),
            )
            .route(
                "/api/banking/provider/pre-evaluations/from-client",
                post(pre_evaluation::create_from_client),
            )
            .route(
                "/api/banking/provider/pre-evaluations/challenge",
                post(pre_evaluation::issue_challenge),
            )
            .route(
                "/api/banking/provider/pre-evaluations/from-evidence",
                post(pre_evaluation::create_from_evidence)
                    .layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
            )
            .route(
                "/internal/v1/technical-pre-evaluations/from-client",
                post(pre_evaluation::create_from_client),
            )
            .route(
                "/internal/v1/technical-pre-evaluations/challenge",
                post(pre_evaluation::issue_challenge),
            )
            .route(
                "/internal/v1/technical-pre-evaluations/from-evidence",
                post(pre_evaluation::create_from_evidence)
                    .layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
            )
            .route(
                "/api/banking/provider/pre-evaluations/:report_id",
                get(pre_evaluation::get_report),
            )
            .route(
                "/api/banking/provider/pre-evaluations/:report_id/html",
                get(pre_evaluation::get_report_html),
            )
            .route(
                "/api/banking/provider/pre-evaluations/:report_id/evidence",
                delete(pre_evaluation::purge_report_evidence),
            )
            .route(
                "/internal/v1/technical-pre-evaluations/:report_id",
                get(pre_evaluation::get_internal_report),
            )
            .route(
                "/internal/v2/technical-snapshots/:snapshot_id",
                get(technical_snapshot::get_internal_snapshot),
            );

        Router::new()
            //user APIs
            // .route("/api/users", get(client::get_users))
            // .route("/api/user/tokens", get(client::get_tokens))
            // //client APIs
            .route("/api/user/insert_client", post(client::insert_client))
            .route("/api/user/client_list", get(client::get_user_clients))
            .route(
                "/api/user/client_device_detail",
                get(client::get_client_detail),
            )
            .route("/api/user/edit_client_info", post(client::edit_client_info))
            .route(
                "/api/user/client_status_list",
                get(client::get_user_client_status_list),
            )
            //client Monitoring
            .route("/api/user/client_stat", get(client::get_client_stats))
            .route("/api/user/client_monitor", get(client::get_client_monitor))
            .route("/api/user/client_health", get(client::get_client_health))
            // Model Download Progress
            .route(
                "/api/user/model_download_progress",
                get(client::get_model_download_progress),
            )
            // Public compute map API
            .route("/api/compute-map", get(compute_map::get_compute_map))
            // Banking admin dashboard APIs
            .route(
                "/api/banking/admin/overview",
                get(banking_admin::get_overview),
            )
            .route(
                "/api/banking/admin/network-map",
                get(banking_admin::get_network_map),
            )
            .route(
                "/api/banking/admin/compute-nodes",
                get(banking_admin::get_compute_nodes),
            )
            .route(
                "/api/banking/admin/token-throughput",
                get(banking_admin::get_token_throughput),
            )
            // Model Management APIs
            .route("/api/models/insert", post(models::create_or_update_model))
            .route("/api/models/get", get(models::get_models))
            // Points Management APIs
            .route("/api/user/points", get(points::get_user_points))
            // APK Management APIs
            .route("/api/apk/upsert", post(apk::upsert_apk))
            .route("/api/apk/get", get(apk::get_apk))
            .route("/api/apk/list", get(apk::list_apk))
            .layer(CorsLayer::permissive())
            .merge(pre_evaluation_routes)
            .with_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use redis::Client as RedisClient;
    use sqlx::postgres::PgPoolOptions;
    use tower::Service;

    fn test_server() -> Arc<ApiServer> {
        let db_pool = PgPoolOptions::new()
            .connect_lazy("postgres://assessment:assessment@127.0.0.1:1/assessment")
            .unwrap();
        let redis_client = Arc::new(RedisClient::open("redis://127.0.0.1:1/").unwrap());
        Arc::new(ApiServer {
            db_pool,
            redis_client,
        })
    }

    #[tokio::test]
    async fn internal_pre_evaluation_create_routes_are_registered() {
        let mut router = test_server().create_api_router().await;
        let cases = [
            (
                "/internal/v1/technical-pre-evaluations/from-client",
                r#"{"gpufUserRef":"user-1","gpufClientRef":"00112233445566778899aabbccddeeff"}"#,
            ),
            ("/internal/v1/technical-pre-evaluations/challenge", ""),
            (
                "/internal/v1/technical-pre-evaluations/from-evidence",
                r#"{"hardwareEvidenceJson":"{}"}"#,
            ),
        ];

        for (path, body) in cases {
            let request = Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            let response = router.call(request).await.unwrap();
            assert!(
                matches!(
                    response.status(),
                    StatusCode::UNAUTHORIZED | StatusCode::SERVICE_UNAVAILABLE
                ),
                "{path}: {}",
                response.status()
            );
        }
    }

    #[tokio::test]
    async fn internal_banking_device_candidates_route_requires_service_auth() {
        let mut router = test_server().create_api_router().await;
        let request = Request::builder()
            .method("GET")
            .uri("/internal/v1/banking/device-candidates?gpufUserRef=1")
            .body(Body::empty())
            .unwrap();
        let response = router.call(request).await.unwrap();
        assert!(matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::SERVICE_UNAVAILABLE
        ));
    }
}
