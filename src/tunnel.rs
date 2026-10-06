//! Startet cloudflared und zeigt Link und QR-Code, sobald der Tunnel steht.

use crate::state::Shared;
use crate::terminal::{field, log, print_qr};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};

/// Hält den Tunnel am Laufen. Bricht er ab, wird er nach 2 Sekunden neu gestartet (mit neuem Link).
pub async fn run(game: Shared, port: u16) {
    loop {
        let mut cmd = Command::new("cloudflared");
        cmd.args(["tunnel", "--no-autoupdate", "--url"]).arg(format!("http://127.0.0.1:{port}"));
        // Eigene, unsichtbare Konsole: sonst verstellt das Programm unser Terminal
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        let spawned = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                eprintln!("\n  cloudflared nicht gefunden ({e}).");
                eprintln!("  Installieren mit: winget install Cloudflare.cloudflared");
                eprintln!("  Danach ein NEUES Terminal öffnen und erneut starten.\n");
                return;
            }
        };
        tokio::spawn(watch(child.stdout.take().unwrap(), game.clone()));
        tokio::spawn(watch(child.stderr.take().unwrap(), game.clone()));

        let _ = child.wait().await;
        game.lock().unwrap().tunnel_url = None;
        log("Tunnel getrennt. Neuer Link folgt, bitte erneut verschicken.");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Liest die Ausgabe von cloudflared und zeigt den Link, sobald er auftaucht.
async fn watch<S: AsyncRead + Unpin>(stream: S, game: Shared) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match find_link(&line) {
            Some(url) => {
                game.lock().unwrap().tunnel_url = Some(url.clone());
                print_qr(&url);
                field("Link", &url);
                println!();
                log("Tunnel steht. QR-Code scannen oder Link verschicken.");
            }
            None if line.contains(" ERR ") => eprintln!("[tunnel] {line}"),
            None => {}
        }
    }
}

fn find_link(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || ".-:/".contains(*c))
        .collect();
    (url.ends_with(".trycloudflare.com") && !url.starts_with("https://api.")).then_some(url)
}
