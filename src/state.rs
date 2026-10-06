//! Spielzustand: Spieler, Regeln (Sperren, Tags, Kicks) und alles, was der Server daraus berechnet.
//! Alles liegt in einer einzigen `Game`-Struktur hinter einem Mutex. Gesperrt wird nur kurz und nie über ein `await`.

use crate::terminal::{dur, log};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_DELAY: u64 = 900; // Sekunden (15 min)
const ONLINE_MS: u64 = 30_000; // so lange gilt jemand als aktiv
const LEAVE_GRACE_MS: u64 = 10_000; // Schonfrist nach Schließen der Seite (z. B. Neuladen)
const IDLE_MS: u64 = 120_000; // so lange ohne Meldung, dann fliegt der Spieler raus
const KEEP_TRACK_MS: u64 = 2 * 3600 * 1000; // so lange wird der Positionsverlauf gespeichert

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

// ---------- Daten ----------

pub struct Point {
    pub t: u64, // Unix-Zeit in ms
    pub lat: f64,
    pub lon: f64,
    pub acc: f64,
}

pub struct Player {
    pub name: String,
    pub delay: u64,           // Sekunden: so verzögert sehen die anderen diesen Spieler
    pub left_at: Option<u64>, // Seite geschlossen (Schonfrist läuft)
    pub seen: u64,            // letzte Meldung, auch ohne Standort
    pub track: Vec<Point>,
}

/// Alles, was der Server weiß. Namen sind in Regeln immer klein geschrieben.
#[derive(Default)]
pub struct Game {
    pub players: HashMap<String, Player>, // Schlüssel = Client-ID (nicht der Name)
    pub satellite: bool,                  // Karte für alle
    pub sat_names: HashMap<String, bool>, // Karte einzelner Spieler (gilt vor `satellite`)
    pub show_delays: bool,                // Änderungen der Verzögerung im Terminal zeigen
    pub lock_all: bool,                   // Verzögerung für alle gesperrt
    pub locks: HashSet<String>,           // Einzelsperren: "name:max" oder "tag:jaeger"
    pub tags: BTreeSet<(String, String)>, // (Name, Tag); bleiben auch nach erneutem Beitritt
    pub kicked: Vec<(String, String)>,    // (Client-ID, Name) der gekickten Spieler
    pub tunnel_url: Option<String>,
}

pub type Shared = Arc<Mutex<Game>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(Game { satellite: true, ..Default::default() }))
}

// ---------- Anfragen der Handys ----------

#[derive(Deserialize)]
pub struct Update {
    pub id: String,
    pub name: String,
    pub lat: Option<f64>, // fehlt, solange das Handy noch keinen Standort hat
    pub lon: Option<f64>,
    pub acc: Option<f64>,
    pub delay: Option<u64>, // nur, wenn der Spieler sie selbst ändert
}

/// Antwort an das Handy: gültige Verzögerung und ob sie gesperrt ist.
#[derive(Serialize)]
pub struct Reply {
    pub delay: u64,
    pub locked: bool,
}

#[derive(Serialize)]
pub struct PlayerView {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub acc: f64,
    pub age_s: u64, // wie alt die angezeigte Position ist
}

#[derive(Serialize)]
pub struct Overview {
    pub online: usize, // alle aktiven Spieler inkl. dir selbst
    pub satellite: bool,
    pub players: Vec<PlayerView>,
}

impl Game {
    // ---------- Regeln abfragen ----------

    pub fn tags_of(&self, name: &str) -> Vec<String> {
        let n = name.to_lowercase();
        self.tags.iter().filter(|(x, _)| *x == n).map(|(_, t)| t.clone()).collect()
    }

    pub fn is_locked(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        self.lock_all
            || self.locks.contains(&format!("name:{n}"))
            || self.tags_of(&n).iter().any(|t| self.locks.contains(&format!("tag:{t}")))
    }

    pub fn is_kicked(&self, id: &str, name: &str) -> bool {
        let n = name.to_lowercase();
        self.kicked.iter().any(|(i, x)| i == id || (!n.is_empty() && *x == n))
    }

    fn sat_for(&self, name: &str) -> bool {
        self.sat_names.get(&name.to_lowercase()).copied().unwrap_or(self.satellite)
    }

    /// Ziel eines Befehls ("alle", "#tag" oder ein Name) als Namen in Kleinbuchstaben.
    pub fn resolve(&self, target: &str) -> Vec<String> {
        let t = target.trim().to_lowercase();
        if t == "alle" || t == "all" {
            self.players.values().map(|p| p.name.to_lowercase()).collect()
        } else if let Some(tag) = t.strip_prefix('#') {
            self.tags.iter().filter(|(_, g)| g == tag).map(|(n, _)| n.clone()).collect()
        } else {
            vec![t.clone()]
        }
    }

    // ---------- Meldungen der Handys verarbeiten ----------

    /// Verarbeitet eine Meldung. `None` heißt: der Spieler wurde gekickt.
    pub fn update(&mut self, u: Update) -> Option<Reply> {
        let name = u.name.trim().to_string();
        if self.is_kicked(&u.id, &name) {
            return None;
        }
        let now = now_ms();

        if !self.players.contains_key(&u.id) {
            // Gleicher Name, alter Eintrag seit >30 s still: derselbe Spieler hat neu verbunden
            // (Seite neu geladen). Dann den Verlauf übernehmen und nichts melden.
            let old = self
                .players
                .iter()
                .find(|(_, p)| p.name == name && now.saturating_sub(p.seen) > ONLINE_MS)
                .map(|(id, _)| id.clone());
            let player = match old.and_then(|id| self.players.remove(&id)) {
                Some(p) => p,
                None => {
                    log(&format!("{name} ist dabei"));
                    Player { name: name.clone(), delay: 0, left_at: None, seen: now, track: Vec::new() }
                }
            };
            self.players.insert(u.id.clone(), player);
        }

        let locked = self.is_locked(&name);
        let show = self.show_delays;
        let p = self.players.get_mut(&u.id)?;
        p.name = name;
        p.left_at = None; // meldet sich wieder: bleibt im Spiel
        p.seen = now;

        // Gesperrt: der Spieler kann sie nicht ändern, nur der Spielleiter
        if let Some(d) = u.delay.map(|d| d.min(MAX_DELAY)) {
            if !locked && d != p.delay {
                if show {
                    log(&if d == 0 {
                        format!("{} zeigt sich wieder live", p.name)
                    } else {
                        format!("{} zeigt sich {} verzögert", p.name, dur(d))
                    });
                }
                p.delay = d;
            }
        }
        if let (Some(lat), Some(lon)) = (u.lat, u.lon) {
            p.track.push(Point { t: now, lat, lon, acc: u.acc.unwrap_or(0.0) });
            p.track.retain(|x| x.t + KEEP_TRACK_MS > now);
        }
        Some(Reply { delay: p.delay, locked })
    }

    /// Seite wurde geschlossen: kurze Schonfrist, falls sie nur neu geladen wird.
    pub fn leave(&mut self, id: &str) {
        if let Some(p) = self.players.get_mut(id.trim()) {
            p.left_at = Some(now_ms());
        }
    }

    /// Entfernt Spieler, die die Seite geschlossen haben oder sich nicht mehr melden.
    pub fn cleanup(&mut self) {
        let now = now_ms();
        let gone: Vec<(String, String, bool)> = self
            .players
            .iter()
            .filter_map(|(id, p)| {
                let closed = p.left_at.map_or(false, |t| now.saturating_sub(t) > LEAVE_GRACE_MS);
                let idle = now.saturating_sub(p.seen) > IDLE_MS;
                (closed || idle).then(|| (id.clone(), p.name.clone(), closed))
            })
            .collect();
        for (id, name, closed) in gone {
            self.players.remove(&id);
            log(&if closed {
                format!("{name} hat das Spiel verlassen")
            } else {
                format!("{name} meldet sich nicht mehr und wurde entfernt")
            });
        }
    }

    // ---------- Was ein Spieler auf seiner Karte sieht ----------

    pub fn overview(&self, me: &str) -> Overview {
        let now = now_ms();
        let players = self
            .players
            .iter()
            .filter(|(id, _)| id.as_str() != me)
            .filter_map(|(id, p)| {
                // Jeder Spieler bestimmt selbst, wie verzögert ihn die anderen sehen
                let cutoff = now.saturating_sub(p.delay * 1000);
                let pt = p.track.iter().rev().find(|x| x.t <= cutoff)?;
                Some(PlayerView {
                    id: id.clone(),
                    name: p.name.clone(),
                    lat: pt.lat,
                    lon: pt.lon,
                    acc: pt.acc,
                    age_s: (now - pt.t) / 1000,
                })
            })
            .collect();
        Overview {
            online: self.players.values().filter(|p| now.saturating_sub(p.seen) < ONLINE_MS).count(),
            satellite: self.players.get(me).map_or(self.satellite, |p| self.sat_for(&p.name)),
            players,
        }
    }
}

/// Räumt alle 5 Sekunden auf.
pub async fn janitor(game: Shared) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        game.lock().unwrap().cleanup();
    }
}
