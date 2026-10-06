//! Manhunt: Server für ein Echtzeit-Verfolgungsspiel mit GPS.
//!
//! - `state`    Spieler, Regeln und was der Server daraus berechnet
//! - `web`      Seite und Schnittstelle für die Handys
//! - `commands` Befehle im Terminal (help, kick, lock, tag, ...)
//! - `tunnel`   öffentlicher Link über cloudflared
//! - `terminal` Ausgabe im Terminal (Titel, Protokoll, QR-Code)

mod commands;
mod state;
mod terminal;
mod tunnel;
mod web;

#[tokio::main]
async fn main() {
    let game = state::shared();

    // In der Cloud gibt der Anbieter den Port über PORT vor
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();

    terminal::title();
    terminal::field("Server", &format!("http://localhost:{port}"));

    // Tunnel nur auf dem eigenen PC (nicht in der Cloud, abschaltbar mit NO_TUNNEL=1)
    if std::env::var("NO_TUNNEL").is_err() && std::env::var("PORT").is_err() {
        terminal::field("Tunnel", "wird aufgebaut ...");
        tokio::spawn(tunnel::run(game.clone(), port));
    }
    tokio::spawn(state::janitor(game.clone()));
    tokio::spawn(commands::listen(game.clone()));

    axum::serve(listener, web::router(game)).await.unwrap();
}
