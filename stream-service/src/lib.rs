mod api;
mod middleware;

use hyper::{server::conn::http1, service::service_fn};
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    server::graceful::GracefulShutdown,
};
use middleware::logger::Logger;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tracing::Level;

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

pub struct ServerBuilder {
    socket_addr: SocketAddr,
    tcp_listener: TcpListener,
}

impl ServerBuilder {
    pub async fn new() -> Self {
        let socket_addr = SocketAddr::from(([0, 0, 0, 0], 3000));
        let tcp_listener = TcpListener::bind(socket_addr).await.unwrap();

        Self {
            socket_addr,
            tcp_listener,
        }
    }

    pub fn init_tracing(self, level: Level) -> Self {
        tracing_subscriber::fmt()
            .with_max_level(level)
            .compact()
            .with_file(true)
            .with_line_number(true)
            .with_target(false)
            .init();

        self
    }

    pub async fn run(self) {
        tracing::info!("Listening on http://{}", self.socket_addr);

        let mut http = http1::Builder::new();
        let graceful = GracefulShutdown::new();
        let mut signal = std::pin::pin!(shutdown_signal());

        loop {
            tokio::select! {
                Ok((stream, _addr)) = self.tcp_listener.accept() => {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(api::init_routers);
                    let svc = ServiceBuilder::new().layer_fn(Logger::new).service(svc);
                    let conn = http
                        .timer(TokioTimer::new())
                        .header_read_timeout(tokio::time::Duration::from_secs(1))
                        .serve_connection(io, svc);
                    let fut = graceful.watch(conn);

                    tokio::task::spawn(async move {
                        if let Err(err) = fut.await {
                            tracing::error!("Error serving connection: {:?}", err);
                        }
                    });
                },
                _ = &mut signal => {
                    drop(self.tcp_listener);
                    tracing::info!("graceful shutdown signal received");
                    break;
                }
            }
        }

        tokio::select! {
            _ = graceful.shutdown() => {
                tracing::info!("all connections gracefully closed");
            },
        }
    }
}
