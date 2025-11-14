pub mod handlers;

use axum::{
    routing::{get, post},
    Router,
};
use handlers::SharedWebState;
use tower_http::services::ServeDir;

pub fn create_router(state: SharedWebState) -> Router {
    Router::new()
        .route("/api/config", get(handlers::get_config).post(handlers::update_config))
        .route("/api/status", get(handlers::get_status))
        .nest_service("/", ServeDir::new("static"))
        .with_state(state)
}
