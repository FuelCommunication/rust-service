mod api;
mod middleware;

use hyper_util::rt::TokioTimer;
use middleware::logger::Logger;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tracing::Level;

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

    pub async fn run(self) -> ! {
        tracing::info!("Listening on http://{}", self.socket_addr);

        loop {
            let (stream, _) = self.tcp_listener.accept().await.unwrap();
            let io = hyper_util::rt::TokioIo::new(stream);

            tokio::task::spawn(async move {
                let svc = hyper::service::service_fn(api::init_routers);
                let svc = ServiceBuilder::new().layer_fn(Logger::new).service(svc);
                let mut http = hyper::server::conn::http1::Builder::new();

                if let Err(err) = http
                    .timer(TokioTimer::new())
                    .header_read_timeout(tokio::time::Duration::from_secs(1))
                    .serve_connection(io, svc)
                    .await
                {
                    tracing::error!("Error serving connection: {:?}", err);
                }
            });
        }
    }
}
