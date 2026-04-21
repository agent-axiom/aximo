use axum::http::StatusCode;

pub async fn ready() -> StatusCode {
    StatusCode::OK
}
