use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use qrcode::{Color, EcLevel, QrCode};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::Mutex,
};

#[derive(Clone)]
struct Point {
    t: u64, // Unix-Zeit in ms
    lat: f64,
    lon: f64,
    acc: f64,
}

struct Player {
    name: String,
    delay: u64, // Sekunden: so verzögert sehen andere diesen Spieler
    track: Vec<Point>,
}

// Schlüssel = eindeutige Client-ID (nicht der Name!)
type Db = Arc<Mutex<HashMap<String, Player>>>;

#[derive(Deserialize)]
struct Update {
    id: String,
    name: String,
    lat: f64,
    lon: f64,
    acc: f64,
    delay: Option<u64>,
}

#[derive(Deserialize)]
struct PlayersQuery {
    me: Option<String>, // eigene Client-ID (wird ausgeblendet)
}

#[derive(Serialize)]
struct PlayersResponse {
    online: usize, // alle aktiven Spieler inkl. dir selbst
    players: Vec<PlayerView>,
}

#[derive(Serialize)]
struct PlayerView {
    id: String,
    name: String,
    lat: f64,
    lon: f64,
    acc: f64,
    age_s: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn icon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        include_bytes!("../static/icon.png").as_slice(),
    )
}

async fn manifest() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        include_str!("../static/manifest.webmanifest"),
    )
}

async fn update(State(db): State<Db>, Json(u): Json<Update>) -> StatusCode {
    let name = u.name.trim().to_string();
    if u.id.is_empty() || u.id.len() > 64 || name.is_empty() || name.len() > 30 {
        return StatusCode::BAD_REQUEST;
    }
    let now = now_ms();
    let mut db = db.lock().await;
    if !db.contains_key(&u.id) {
        // Gleicher Name, alter Eintrag seit >30 s still = derselbe Spieler hat neu
        // verbunden (Seite neu geladen): Verlauf übernehmen, nichts melden.
        let old = db
            .iter()
            .find(|(_, p)| {
                p.name == name
                    && p.track.last().map_or(true, |x| now.saturating_sub(x.t) > 30_000)
            })
            .map(|(k, _)| k.clone());
        match old {
            Some(k) => {
                let p = db.remove(&k).unwrap();
                db.insert(u.id.clone(), p);
            }
            None => println!("  {G}+{R} {name} ist beigetreten"),
        }
    }
    let p = db.entry(u.id).or_insert_with(|| Player {
        name: name.clone(),
        delay: 0,
        track: Vec::new(),
    });
    p.name = name;
    p.delay = u.delay.unwrap_or(0).min(3600);
    p.track.push(Point { t: now, lat: u.lat, lon: u.lon, acc: u.acc });
    p.track.retain(|x| x.t + 2 * 3600 * 1000 > now); // nur 2 Std. Verlauf
    StatusCode::OK
}

async fn players(
    State(db): State<Db>,
    Query(q): Query<PlayersQuery>,
) -> Json<PlayersResponse> {
    let now = now_ms();
    let me = q.me.unwrap_or_default();

    let db = db.lock().await;
    let mut out = Vec::new();
    for (id, player) in db.iter() {
        if *id == me {
            continue;
        }
        // Jeder Spieler bestimmt selbst, wie verzögert die anderen ihn sehen
        let cutoff = now.saturating_sub(player.delay * 1000);
        if let Some(p) = player.track.iter().rev().find(|p| p.t <= cutoff) {
            out.push(PlayerView {
                id: id.clone(),
                name: player.name.clone(),
                lat: p.lat,
                lon: p.lon,
                acc: p.acc,
                age_s: (now - p.t) / 1000,
            });
        }
    }
    let online = db
        .values()
        .filter(|p| p.track.last().map_or(false, |x| now.saturating_sub(x.t) < 30_000))
        .count();
    Json(PlayersResponse { online, players: out })
}

// ---------- Automatischer Tunnel (localhost.run) + QR-Seite ----------

const B: &str = "\x1b[1m";
const G: &str = "\x1b[32m";
const Y: &str = "\x1b[33m";
const C: &str = "\x1b[36m";
const R: &str = "\x1b[0m";

fn banner() {
    println!();
    println!("  {C}{B}┌──────────────────────────────────┐{R}");
    println!("  {C}{B}│        M A N H U N T   LIVE      │{R}");
    println!("  {C}{B}└──────────────────────────────────┘{R}");
    println!();
}

fn status(label: &str, value: &str, color: &str) {
    println!("  {color}●{R} {B}{label:<9}{R} {value}");
}

fn print_qr(url: &str) {
    // Niedrige Fehlerkorrektur = kleinerer Code, passt besser ins Terminal
    let Ok(code) = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L) else {
        return;
    };
    let w = code.width();
    let colors = code.to_colors();
    let q = 2usize; // weißer Rand
    let mut out = String::new();
    for y in 0..(w + 2 * q) {
        out.push_str("  ");
        for x in 0..(w + 2 * q) {
            let inside = x >= q && y >= q && x < w + q && y < w + q;
            let dark = inside && colors[(y - q) * w + (x - q)] == Color::Dark;
            // 2 Zeichen pro Modul (quadratisch). Für dunkle Terminals invertiert.
            out.push_str(if dark { "  " } else { "██" });
        }
        out.push('\n');
    }
    print!("\n{out}\n");
}

async fn read_lines<R: AsyncRead + Unpin>(r: R) {
    let mut lines = BufReader::new(r).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.contains("tunneled with") {
            if let Some(i) = line.find("https://") {
                let url: String = line[i..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || ".-:/".contains(*c))
                    .collect();
                print_qr(&url);
                status("Link", &url, G);
                println!("\n  QR-Code scannen oder Link verschicken. Warte auf Mitspieler ...\n");
            }
        } else if line.starts_with("Permission denied")
            || line.contains("Could not resolve")
            || line.contains("Connection refused")
        {
            eprintln!("[ssh] {line}");
        }
    }
}

async fn run_tunnel() {
    loop {
        let mut cmd = Command::new("ssh");
        // Eigene, unsichtbare Konsole für ssh: sonst verstellt ssh unser Terminal
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let spawned = cmd
            .args([
                "-o", "StrictHostKeyChecking=accept-new",
                "-o", "ServerAliveInterval=30",
                "-T",
                "-R", "80:127.0.0.1:3000",
                "nokey@localhost.run",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ssh konnte nicht gestartet werden: {e}");
                return;
            }
        };
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        tokio::spawn(read_lines(out));
        tokio::spawn(read_lines(err));

        let _ = child.wait().await;
        println!("\n  {Y}●{R} Tunnel getrennt, neuer Versuch in 5 s ...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[tokio::main]
async fn main() {
    let db: Db = Arc::new(Mutex::new(HashMap::new()));

    let app = Router::new()
        .route("/", get(index))
        .route("/icon.png", get(icon))
        .route("/manifest.webmanifest", get(manifest))
        .route("/api/update", post(update))
        .route("/api/players", get(players))
        .with_state(db);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    banner();
    status("Server", "http://localhost:3000", G);

    // Tunnel automatisch starten (abschalten mit Umgebungsvariable NO_TUNNEL=1)
    if std::env::var("NO_TUNNEL").is_err() {
        status("Tunnel", "wird gestartet ...", Y);
        tokio::spawn(run_tunnel());
    }

    axum::serve(listener, app).await.unwrap();
}
