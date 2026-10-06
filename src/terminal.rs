//! Ausgabe im Terminal: Titel, Protokollzeilen, beschriftete Felder und QR-Code.

use qrcode::{Color, EcLevel, QrCode};
use std::{sync::OnceLock, time::Instant};

// Farben (ANSI-Steuerzeichen)
pub const B: &str = "\x1b[1m"; // fett
pub const D: &str = "\x1b[2m"; // gedämpft
pub const R: &str = "\x1b[0m"; // zurücksetzen
const RED: &str = "\x1b[91m";

/// Großer Titel beim Start (78 Zeichen breit)
const TITLE: [&str; 7] = [
    "░███     ░███                       ░██                                 ░██",
    "░████   ░████                       ░██                                 ░██",
    "░██░██ ░██░██  ░██████   ░████████  ░████████  ░██    ░██ ░████████  ░████████",
    "░██ ░████ ░██       ░██  ░██    ░██ ░██    ░██ ░██    ░██ ░██    ░██    ░██",
    "░██  ░██  ░██  ░███████  ░██    ░██ ░██    ░██ ░██    ░██ ░██    ░██    ░██",
    "░██       ░██ ░██   ░██  ░██    ░██ ░██    ░██ ░██   ░███ ░██    ░██    ░██",
    "░██       ░██  ░█████░██ ░██    ░██ ░██    ░██  ░█████░██ ░██    ░██     ░████",
];

static START: OnceLock<Instant> = OnceLock::new();

/// Zeigt den Titel und startet die Spielzeit, die `log` anzeigt.
pub fn title() {
    START.get_or_init(Instant::now);
    println!();
    for line in TITLE {
        println!("{RED}  {line}{R}");
    }
    println!("\n  {D}Jagd mit Live-Standort{R}\n");
}

/// Protokollzeile mit Spielzeit seit Serverstart (Minuten:Sekunden).
pub fn log(msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs();
    println!("  {D}{:>3}:{:02}{R}  {msg}", t / 60, t % 60);
}

/// Beschriftete Angabe, z. B. "Link     https://..."
pub fn field(label: &str, value: &str) {
    println!("  {D}{label:<8}{R}{B}{value}{R}");
}

/// Sekunden als Text: "40 s", "5 min", "1 min 30 s".
pub fn dur(s: u64) -> String {
    match s {
        0..=59 => format!("{s} s"),
        _ if s % 60 == 0 => format!("{} min", s / 60),
        _ => format!("{} min {} s", s / 60, s % 60),
    }
}

/// Zeichnet einen QR-Code aus Blockzeichen (2 Zeichen pro Feld, für dunkle Terminals invertiert).
pub fn print_qr(url: &str) {
    // Niedrige Fehlerkorrektur = kleinerer Code, passt besser ins Terminal
    let Ok(code) = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L) else {
        return;
    };
    let (w, margin) = (code.width(), 2);
    let size = w + 2 * margin;
    let dark = code.to_colors();
    let inside = margin..w + margin;

    let mut out = String::new();
    for y in 0..size {
        out.push_str("  ");
        for x in 0..size {
            let on = inside.contains(&x)
                && inside.contains(&y)
                && dark[(y - margin) * w + (x - margin)] == Color::Dark;
            out.push_str(if on { "  " } else { "██" });
        }
        out.push('\n');
    }
    print!("\n{out}\n");
}
