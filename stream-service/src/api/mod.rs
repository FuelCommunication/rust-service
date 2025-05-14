use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::{Request, Response, StatusCode, header, Method, body::{Bytes, Incoming}};

pub(super) type ApiResult<T> = Result<T, hyper::Error>;
pub(super) type BoxBodyResult = BoxBody<Bytes, hyper::Error>;

pub(super) async fn init_routers(
    req: Request<Incoming>,
) -> ApiResult<Response<BoxBody<Bytes, hyper::Error>>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/ping") => ping().await,
        _ => Ok(not_found()),
    }
}

fn empty() -> BoxBodyResult {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBodyResult {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

pub(crate) async fn ping() -> ApiResult<Response<BoxBodyResult>> {
    let json = serde_json::json!({"ping": "pong"}).to_string();

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(full(json))
        .unwrap();

    Ok(response)
}

pub(crate) fn not_found() -> Response<BoxBodyResult> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(empty())
        .unwrap()
}
