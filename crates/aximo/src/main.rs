use aximo::{app::build_app, config::Settings};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("bind listener");
    let app = build_app(Settings::default());

    axum::serve(listener, app).await.expect("serve aximo");
}
