use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::{Request, Response, StatusCode, header};

pub(super) type ApiResult<T> = Result<T, hyper::Error>;
pub(super) type BoxBodyResult = BoxBody<Bytes, hyper::Error>;

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

pub(crate) async fn ping(_: Request<hyper::body::Incoming>) -> ApiResult<Response<BoxBodyResult>> {
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
