use std::sync::Arc;

use aximo::{app::build_app, config::Settings};
use aximo_inference::engine::UnavailableEngine;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("bind listener");
    let app = build_app(
        Settings::default(),
        Arc::new(UnavailableEngine::new(
            "no runtime speech engine configured yet",
        )),
    );

    axum::serve(listener, app).await.expect("serve aximo");
}
