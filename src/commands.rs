//! Befehle, die der Spielleiter im Terminal eintippt.
//! Ein "Ziel" ist `alle`, ein Name (`max`) oder ein Tag (`#jaeger`).

use crate::state::{now_ms, Game, Player, Shared, MAX_DELAY};
use crate::terminal::{dur, field, log, print_qr, B, D, R};
use std::collections::BTreeMap;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Liest Zeile für Zeile aus dem Terminal und führt die Befehle aus.
pub async fn listen(game: Shared) {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        run(&game, &line);
    }
}

fn run(game: &Shared, line: &str) {
    let words: Vec<String> = line.split_whitespace().map(str::to_lowercase).collect();
    let Some((cmd, rest)) = words.split_first() else { return };
    let arg = rest.first().map(String::as_str);
    let who = rest.join(" ");
    let mut g = game.lock().unwrap();

    match cmd.as_str() {
        "help" | "hilfe" | "?" => help(&g),
        "spieler" | "players" => list_players(&g),
        "tags" => list_tags(&g),

        "verzoegerung" | "verzögerung" => match switch(arg, g.show_delays) {
            Some(on) => {
                g.show_delays = on;
                log(if on {
                    "Änderungen der Verzögerung werden angezeigt"
                } else {
                    "Änderungen der Verzögerung werden nicht mehr angezeigt"
                });
            }
            None => println!("  Benutzung: verzoegerung an|aus"),
        },

        "sat" | "satellit" | "satellite" => {
            let target = rest.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            let map_name = |on: bool| if on { "Satellitenkarte an" } else { "Normale Karte an" };
            match switch(arg, g.satellite) {
                Some(on) if target.is_empty() => {
                    g.satellite = on;
                    g.sat_names.clear();
                    log(&format!("{} für alle Spieler", map_name(on)));
                }
                Some(on) => {
                    let names = g.resolve(&target);
                    if names.is_empty() {
                        println!("  Niemand gefunden für {target}.");
                    } else {
                        log(&format!("{} für {target} ({} Spieler)", map_name(on), names.len()));
                        for n in names {
                            g.sat_names.insert(n, on);
                        }
                    }
                }
                None => println!("  Benutzung: sat an|aus [ziel]   z. B. sat aus #jaeger"),
            }
        }

        "delay" | "delays" => match rest.split_last() {
            Some((time, target)) if !target.is_empty() => match parse_time(time) {
                Some(secs) => set_delay(&mut g, &target.join(" "), secs),
                None => println!("  Zeit nicht verstanden. Beispiele: 5, 90s, live"),
            },
            _ => println!("  Benutzung: delay <ziel> <zeit>   z. B. delay max 5   oder   delay #jaeger 90s"),
        },
        "lock" => set_lock(&mut g, &who, true),
        "unlock" => set_lock(&mut g, &who, false),

        "tag" | "untag" => match rest.split_last() {
            Some((tag, target)) if !target.is_empty() => {
                set_tag(&mut g, &target.join(" "), tag.trim_start_matches('#'), cmd == "tag")
            }
            _ => println!("  Benutzung: {cmd} <name|alle|#tag> <tag>"),
        },

        "kick" | "kicken" if who.is_empty() => println!("  Benutzung: kick <ziel>"),
        "kick" | "kicken" => kick(&mut g, &who),
        "unkick" if who.is_empty() => println!("  Benutzung: unkick <ziel>"),
        "unkick" => {
            let names = g.resolve(&who);
            let before = g.kicked.len();
            g.kicked.retain(|(_, n)| !names.contains(n));
            if g.kicked.len() < before {
                log(&format!("Sperre für {who} aufgehoben"));
            } else {
                println!("  {who} ist nicht gesperrt.");
            }
        }

        "link" | "qr" => match &g.tunnel_url {
            Some(url) => {
                print_qr(url);
                field("Link", url);
                println!();
            }
            None => println!("  Noch kein Tunnel. Kurz warten."),
        },
        "exit" | "quit" | "ende" => std::process::exit(0),
        other => println!("  Unbekannter Befehl: {other}. Mit help siehst du alle Befehle."),
    }
}

// ---------- Hilfsfunktionen ----------

/// "an" / "aus"; ohne Angabe wird umgeschaltet. `None` = nicht verstanden.
fn switch(arg: Option<&str>, current: bool) -> Option<bool> {
    match arg {
        None => Some(!current),
        Some("an" | "on" | "ein") => Some(true),
        Some("aus" | "off") => Some(false),
        _ => None,
    }
}

/// Zeit aus dem Befehl: "live", "5" (Minuten), "5m", "5min", "90s"; höchstens 15 Minuten.
fn parse_time(t: &str) -> Option<u64> {
    let t = t.trim().to_lowercase();
    if t == "live" {
        return Some(0);
    }
    let (num, per_unit) = if let Some(n) = t.strip_suffix("min") {
        (n, 60.0)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 60.0)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1.0)
    } else {
        (t.as_str(), 60.0)
    };
    let v: f64 = num.trim().replace(',', ".").parse().ok()?;
    (v >= 0.0).then(|| ((v * per_unit).round() as u64).min(MAX_DELAY))
}

// ---------- Befehle ----------

fn help(g: &Game) {
    let onoff = |on: bool| if on { "an" } else { "aus" };
    let rows = [
        ("help", "diese Übersicht".to_string()),
        ("spieler", "wer ist dabei, mit Verzögerung".to_string()),
        (
            "verzoegerung an|aus",
            format!("Änderungen der Verzögerung live anzeigen (jetzt {})", onoff(g.show_delays)),
        ),
        (
            "sat an|aus [ziel]",
            format!("Satellitenkarte, ohne Ziel für alle (jetzt {})", onoff(g.satellite)),
        ),
        ("delay <ziel> <zeit>", "Verzögerung setzen, z. B. delay max 5".to_string()),
        (
            "lock [ziel]",
            format!(
                "Verzögerung sperren, Spieler können sie nicht ändern (jetzt {})",
                if g.lock_all { "für alle" } else { "nicht für alle" }
            ),
        ),
        ("unlock [ziel]", "Sperre aufheben".to_string()),
        ("tag <ziel> <tag>", "Tag vergeben, mit untag wieder entfernen".to_string()),
        ("tags", "alle Tags zeigen".to_string()),
        ("kick <ziel>", "Spieler entfernen und sperren".to_string()),
        ("unkick <ziel>", "Kick aufheben".to_string()),
        ("link", "QR-Code und Link noch einmal zeigen".to_string()),
        ("exit", "Server beenden".to_string()),
    ];
    println!();
    for (cmd, text) in rows {
        println!("  {B}{cmd:<21}{R}{D}{text}{R}");
    }
    println!(
        "\n  {D}Ziel ist alle, ein Name oder #tag. Zeit: 5 (Minuten), 90s oder live.\n  Ohne an oder aus wird umgeschaltet.{R}\n"
    );
}

fn list_players(g: &Game) {
    if g.players.is_empty() {
        println!("  Noch niemand dabei.");
        return;
    }
    let now = now_ms();
    let mut rows: Vec<&Player> = g.players.values().collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    println!();
    for p in rows {
        let ago = now.saturating_sub(p.seen) / 1000;
        let delay = if p.delay == 0 { "live".to_string() } else { format!("{} verzögert", dur(p.delay)) };
        let state = if p.left_at.is_some() {
            "hat die Seite geschlossen".to_string()
        } else if ago > 30 {
            format!("keine Meldung seit {}", dur(ago))
        } else if p.track.is_empty() {
            "dabei, aber ohne Standort".to_string()
        } else {
            "aktiv".to_string()
        };
        let mut extra = String::new();
        if g.is_locked(&p.name) {
            extra.push_str("  gesperrt");
        }
        for t in g.tags_of(&p.name) {
            extra.push_str(&format!("  #{t}"));
        }
        println!("  {B}{:<14}{R}{:<22}{D}{state}{extra}{R}", p.name, delay);
    }
    println!();
}

fn list_tags(g: &Game) {
    if g.tags.is_empty() {
        println!("  Noch keine Tags. Beispiel: tag max jaeger");
        return;
    }
    let mut by_tag: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, tag) in &g.tags {
        by_tag.entry(tag.as_str()).or_default().push(name.as_str());
    }
    println!();
    for (tag, names) in by_tag {
        println!("  {B}#{tag:<14}{R}{D}{}{R}", names.join(", "));
    }
    println!();
}

/// Setzt die Verzögerung; die Handys übernehmen den Wert bei der nächsten Meldung (auch gesperrte).
fn set_delay(g: &mut Game, target: &str, secs: u64) {
    let names = g.resolve(target);
    let mut n = 0;
    for p in g.players.values_mut().filter(|p| names.contains(&p.name.to_lowercase())) {
        p.delay = secs;
        n += 1;
    }
    if n == 0 {
        println!("  Niemand gefunden für {target}. Mit spieler siehst du alle.");
    } else {
        let what = if secs == 0 { "live".to_string() } else { dur(secs) };
        log(&format!("Verzögerung auf {what} gesetzt ({n} Spieler)"));
    }
}

/// Sperrt oder gibt frei: für alle (ohne Ziel), einen Namen oder einen Tag.
fn set_lock(g: &mut Game, target: &str, on: bool) {
    if target.is_empty() || target == "alle" || target == "all" {
        g.lock_all = on;
        if !on {
            g.locks.clear();
        }
        log(if on { "Verzögerung für alle gesperrt" } else { "Verzögerung für alle freigegeben" });
        return;
    }
    let rule = match target.strip_prefix('#') {
        Some(tag) => format!("tag:{tag}"),
        None => format!("name:{target}"),
    };
    if on {
        g.locks.insert(rule);
        log(&format!("Verzögerung für {target} gesperrt"));
    } else if g.locks.remove(&rule) {
        log(&format!("Verzögerung für {target} freigegeben"));
    } else {
        println!("  {target} ist nicht einzeln gesperrt. Mit unlock alle gibst du alles frei.");
    }
}

fn set_tag(g: &mut Game, target: &str, tag: &str, add: bool) {
    let names = g.resolve(target);
    let changed = names
        .into_iter()
        .filter(|n| {
            let pair = (n.clone(), tag.to_string());
            if add { g.tags.insert(pair) } else { g.tags.remove(&pair) }
        })
        .count();
    if changed == 0 {
        println!("  Nichts geändert für {target}.");
    } else {
        log(&format!("Tag #{tag} {} ({changed})", if add { "vergeben" } else { "entfernt" }));
    }
}

/// Entfernt Spieler und sperrt sie (Gerät und Name), bis `unkick` kommt.
fn kick(g: &mut Game, target: &str) {
    let names = g.resolve(target);
    let hits: Vec<(String, String)> = g
        .players
        .iter()
        .filter(|(_, p)| names.contains(&p.name.to_lowercase()))
        .map(|(id, p)| (id.clone(), p.name.clone()))
        .collect();
    if hits.is_empty() {
        println!("  Niemand gefunden für {target}. Mit spieler siehst du alle.");
        return;
    }
    for (id, name) in hits {
        g.players.remove(&id);
        g.kicked.push((id, name.to_lowercase()));
        log(&format!("{name} wurde gekickt und gesperrt"));
    }
}
