use hyper::{Request, body::Incoming, service::Service};

#[derive(Debug, Clone)]
pub struct Logger<S> {
    inner: S,
}
impl<S> Logger<S> {
    pub fn new(inner: S) -> Self {
        Logger { inner }
    }
}
type Req = Request<Incoming>;

impl<S: Service<Req>> Service<Req> for Logger<S> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, req: Req) -> Self::Future {
        tracing::info!("processing request: {} {}", req.method(), req.uri().path());
        self.inner.call(req)
    }
}
