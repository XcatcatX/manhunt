//! Die Web-Schnittstelle: Seite, Icon und die drei Aufrufe der Handys.

use crate::state::{Overview, Reply, Shared, Update};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

pub fn router(game: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/icon.png", get(icon))
        .route("/manifest.webmanifest", get(manifest))
        .route("/api/update", post(update))
        .route("/api/leave", post(leave))
        .route("/api/players", get(players))
        .with_state(game)
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn icon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../static/icon.png").as_slice())
}

async fn manifest() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        include_str!("../static/manifest.webmanifest"),
    )
}

/// Ein Handy meldet sich (mit oder ohne Standort). 403 heißt: gekickt.
async fn update(State(game): State<Shared>, Json(u): Json<Update>) -> Result<Json<Reply>, StatusCode> {
    let name = u.name.trim();
    if u.id.is_empty() || u.id.len() > 64 || name.is_empty() || name.len() > 30 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let reply = game.lock().unwrap().update(u);
    reply.map(Json).ok_or(StatusCode::FORBIDDEN)
}

/// Die Seite wurde geschlossen (der Body ist die Client-ID).
async fn leave(State(game): State<Shared>, id: String) -> StatusCode {
    game.lock().unwrap().leave(&id);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct PlayersQuery {
    me: Option<String>, // eigene Client-ID (wird ausgeblendet)
}

/// Was dieser Spieler auf seiner Karte sieht.
async fn players(
    State(game): State<Shared>,
    Query(q): Query<PlayersQuery>,
) -> Result<Json<Overview>, StatusCode> {
    let me = q.me.unwrap_or_default();
    let game = game.lock().unwrap();
    if game.is_kicked(&me, "") {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Json(game.overview(&me)))
}
