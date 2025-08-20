use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub enum Image {
    Filename(String),
    File(String, Vec<u8>),
}

impl IntoResponse for Image {
    fn into_response(self) -> Response {
        match self {
            Self::Filename(name) => (StatusCode::OK, name).into_response(),
            Self::File(filename, data) => {
                let filename_header_value = format!("attachment; filename=\"{filename}\"");

                Response::builder()
                    .header("Content-Disposition", filename_header_value)
                    .header("Content-Type", "image/jpeg")
                    .body(Body::from(data))
                    .unwrap()
            }
        }
    }
}

impl From<(String, Vec<u8>)> for Image {
    fn from(val: (String, Vec<u8>)) -> Self {
        Image::File(val.0, val.1)
    }
}

impl From<String> for Image {
    fn from(val: String) -> Self {
        Image::Filename(val.to_owned())
    }
}

impl From<&str> for Image {
    fn from(val: &str) -> Self {
        Image::Filename(val.to_owned())
    }
}
