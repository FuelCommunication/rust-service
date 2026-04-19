use pingora::prelude::{Opt, Server, http_proxy_service};
use service_gateway::{Gateway, PingoraResult, config::Config, init_tracing, log_config, parse_upstream};
use std::sync::Arc;
use tonic::transport::Endpoint;

fn main() -> PingoraResult<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = Arc::new(Config::from_env());
    log_config(&config);

    let opt = Opt::parse_args();
    let mut server = Server::new(Some(opt))?;
    {
        let conf = Arc::get_mut(&mut server.configuration).expect("Server configuration should not be shared before bootstrap");
        conf.grace_period_seconds = Some(config.grace_period_secs);
        conf.graceful_shutdown_timeout_seconds = Some(config.graceful_shutdown_timeout_secs);
    }
    server.bootstrap();

    let auth_grpc_uri = format!("http://{}", config.auth_upstream);
    let auth_endpoint: Endpoint = auth_grpc_uri.parse().expect("Failed to parse auth upstream as gRPC endpoint");

    let gateway = Gateway::new(
        parse_upstream(&config.images_upstream),
        parse_upstream(&config.chats_upstream),
        parse_upstream(&config.channels_upstream),
        parse_upstream(&config.calls_upstream),
        parse_upstream(&config.auth_upstream),
        auth_endpoint,
        Arc::clone(&config),
    );

    let mut lb = http_proxy_service(&server.configuration, gateway);
    lb.add_tcp(&config.listen_addr);

    server.add_service(lb);
    server.run_forever();
}
