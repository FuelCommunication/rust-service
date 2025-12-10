use pingora::prelude::{Server, http_proxy_service};
use service_proxy::{ProxyResult, ProxyService};

fn main() -> ProxyResult<()> {
    let mut server = Server::new(None)?;
    server.bootstrap();

    let proxy_service = ProxyService::new();

    let mut proxy = http_proxy_service(&server.configuration, proxy_service);
    proxy.add_tcp("0.0.0.0:8080");

    server.add_service(proxy);
    server.run_forever();
}
