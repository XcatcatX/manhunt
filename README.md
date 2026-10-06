# Manhunt

Echtzeit-Verfolgungsspiel mit GPS. Jeder öffnet die Seite auf dem Handy, der Server zeigt allen die Standorte der anderen.
Jeder Spieler bestimmt selbst, wie verzögert ihn die anderen sehen; der Spielleiter kann das im Terminal festlegen und sperren.

## Starten

    winget install Cloudflare.cloudflared   # einmalig, danach neues Terminal
    cargo run                               # zeigt QR-Code und Link für die Mitspieler

Ohne Tunnel: `NO_TUNNEL=1 cargo run`. Der Port kommt aus `PORT` (Standard 3000).

## Aufbau

    src/main.rs      Start: Server, Tunnel, Aufräumen und Befehle zusammenstecken
    src/state.rs     Spielzustand (Spieler, Sperren, Tags, Kicks) und was der Server daraus berechnet
    src/web.rs       Die drei Aufrufe der Handys: update, leave, players (plus Seite und Icon)
    src/commands.rs  Befehle im Terminal
    src/tunnel.rs    cloudflared starten, Link und QR-Code zeigen
    src/terminal.rs  Ausgabe: Titel, Protokollzeilen, QR-Code
    static/          Webseite (index.html), Icon, Manifest für den Home-Bildschirm

## Befehle (im Terminal tippen, `help` zeigt alles)

Ein Ziel ist `alle`, ein Name (`max`) oder ein Tag (`#jaeger`).

    spieler                  wer ist dabei
    delay <ziel> <zeit>      Verzögerung setzen (5 = Minuten, 90s, live)
    lock [ziel] / unlock     Verzögerung sperren / freigeben
    tag <ziel> <tag>         Tag vergeben (untag entfernt ihn), tags zeigt alle
    sat an|aus [ziel]        Satellitenkarte
    kick <ziel> / unkick     entfernen und sperren / Sperre aufheben
    verzoegerung an|aus      Änderungen der Verzögerung im Terminal zeigen
    link                     QR-Code und Link noch einmal zeigen
