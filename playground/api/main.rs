use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::IntoMakeServiceWithConnectInfo;
use runfiles::Runfiles;
use tokio::net::TcpListener;

use playground__api::routes::build_router;
use playground__api::session_store::SessionStore;

#[tokio::main]
async fn main() {
    let bind_address =
        std::env::var("PLAYGROUND_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let session_root = std::env::var("PLAYGROUND_SESSION_ROOT").map_or_else(
        |_| std::env::temp_dir().join("coppice-playground"),
        PathBuf::from,
    );
    let web_root = resolve_web_root_from_runfiles();

    let session_store = Arc::new(SessionStore::new(session_root));
    let app = build_router(session_store.clone(), web_root);

    let cleanup_store = session_store.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            cleanup_store.cleanup_expired(Duration::from_secs(3600));
        }
    });

    let listener = TcpListener::bind(&bind_address)
        .await
        .unwrap_or_else(|error| panic!("failed to bind {bind_address}: {error}"));
    let local_address: SocketAddr = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("failed to read local address: {error}"));
    eprintln!("playground api listening on http://{local_address}");

    let make_service: IntoMakeServiceWithConnectInfo<_, SocketAddr> =
        app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, make_service)
        .await
        .unwrap_or_else(|error| panic!("api server failed: {error}"));
}

fn resolve_web_root_from_runfiles() -> PathBuf {
    let runfiles = Runfiles::create().unwrap_or_else(|error| {
        panic!("failed to initialize runfiles for playground web: {error}")
    });
    let index_path = runfiles
        .rlocation("_main/playground/web/index.html")
        .unwrap_or_else(|| panic!("failed to resolve runfiles path for playground/web/index.html"));
    index_path.parent().map_or_else(
        || {
            panic!(
                "runfiles index path has no parent: {}",
                index_path.display()
            )
        },
        PathBuf::from,
    )
}
