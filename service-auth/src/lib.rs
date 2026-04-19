pub mod config;
pub mod error;
pub mod oauth;
pub mod service;
pub mod state;
pub mod store;
pub mod token;

use config::Config;
use mimalloc::MiMalloc;
use oauth::OAuthManager;
use proto::auth_service_server::AuthServiceServer;
use service::AuthServiceImpl;
use sqlx::postgres::PgPoolOptions;
use state::{ServerData, ServerState};
use std::sync::Arc;
use std::time::Duration;
use store::AuthStore;
use token::TokenManager;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub mod proto {
    tonic::include_proto!("auth");
}

pub struct ServerBuilder {
    config: Config,
    state: ServerState,
}

impl ServerBuilder {
    pub async fn new() -> Self {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .compact()
            .with_file(true)
            .with_line_number(true)
            .with_target(false)
            .init();

        let config = Config::from_env();
        let state = Self::init_state(&config).await;
        Self { config, state }
    }

    async fn init_state(config: &Config) -> ServerState {
        let pool = PgPoolOptions::new()
            .max_connections(config.db_max_connections)
            .min_connections(config.db_min_connections)
            .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
            .connect(&config.database_url)
            .await
            .expect("Failed to connect to PostgreSQL");

        let store = AuthStore::new(pool);
        let tokens = Arc::new(TokenManager::new(
            &config.jwt_secret,
            config.jwt_access_expiration_secs,
            config.jwt_refresh_expiration_secs,
        ));
        let oauth = OAuthManager::new(config);
        Arc::new(ServerData { store, tokens, oauth })
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("0.0.0.0:{}", self.config.grpc_port).parse()?;
        let svc = AuthServiceImpl::new(Arc::clone(&self.state));

        let cancel = CancellationToken::new();
        let cleanup_state = Arc::clone(&self.state);
        let cleanup_cancel = cancel.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        match cleanup_state.store.cleanup_expired_tokens().await {
                            Ok(count) if count > 0 => {
                                tracing::info!(deleted = count, "Cleaned up expired refresh tokens");
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to clean up expired tokens");
                            }
                            _ => {}
                        }
                    }
                    _ = cleanup_cancel.cancelled() => break,
                }
            }
        });

        tracing::info!(%addr, "gRPC auth server starting");

        let shutdown_cancel = cancel.clone();
        Server::builder()
            .add_service(AuthServiceServer::new(svc))
            .serve_with_shutdown(addr, async move {
                shutdown_signal().await;
                shutdown_cancel.cancel();
            })
            .await?;

        self.state.store.pool().close().await;
        tracing::info!("Database pool closed");

        Ok(())
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received Ctrl+C"),
        _ = terminate => tracing::info!("Received SIGTERM"),
    }

    tracing::info!("Starting graceful shutdown");
}
