# LLM Bridge

Lokaler Mini-Proxy für macOS, Windows und Linux, der **OpenAI-Chat-Completions** in **Anthropic-Messages** übersetzt. Damit können Tools, die nur „OpenAI-kompatible“ Anbieter kennen – etwa MacWhisper –, Claude-Modelle über **LangDock** oder **Anthropic direkt** nutzen. Jede Anfrage wird mit ihrem Token-Verbrauch in einer lokalen Statistik erfasst.

```
MacWhisper ──OpenAI──▶ LLM Bridge (127.0.0.1:4000/v1) ──Anthropic──▶ LangDock /anthropic/eu/v1/messages
                              │
                              └─▶ SQLite-Statistik (nur Metadaten, keine Inhalte)
```

## Funktionen

- OpenAI-Endpunkte `/v1/chat/completions` (mit und ohne Streaming) und `/v1/models`
- Übersetzung von System-Prompts, Bildern (Base64), Tools/Function Calling, `stop`, `temperature`, `max_tokens`, JSON-Modus
- SSE-Streaming: Anthropic-Events → OpenAI-Chunks inkl. `stream_options.include_usage`
- Fehler im OpenAI-Format mit verständlicher Meldung (z. B. falsche Modell-ID)
- **Token-Statistik**: Anfragen, Input/Output/Cache-Tokens, Antwortzeiten – je Stunde/Tag, je Modell, je Client (erkannt am User-Agent), CSV-Export
- API-Schlüssel im **System-Schlüsselbund** (macOS Keychain, Windows Anmeldeinformationsverwaltung, Linux Secret Service), alternativ per Umgebungsvariable
- Tray-App (Tauri 2) mit Start/Stopp, Autostart bei Anmeldung, Live-Aktualisierung
- **Mehrsprachig** (Deutsch, Englisch; automatische Erkennung der Systemsprache)
- Headless-CLI für Server und Skripte – teilt Konfiguration und Statistik mit der App

## Projektaufbau

| Pfad | Inhalt |
|---|---|
| `crates/core` | Proxy-Logik als Library: Konfiguration, Übersetzung, Streaming, Statistik, axum-Server |
| `crates/cli` | Binary `llm-bridge` ohne GUI |
| `app/src-tauri` | Tauri-Backend: Tray, Fenster, Befehle für die Oberfläche |
| `app/ui` | Oberfläche (HTML/CSS/JS ohne Build-Schritt), Sprachdateien in `app/ui/locales/` |

## Bauen

Voraussetzungen: Rust ≥ 1.87 und die [Tauri-Systemabhängigkeiten](https://v2.tauri.app/start/prerequisites/). Unter Linux z. B.:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

```bash
cargo install tauri-cli --version "^2" --locked

# App im Entwicklungsmodus
cd app/src-tauri && cargo tauri dev

# Installer (.dmg/.app, .msi/.exe, .deb/.AppImage/.rpm) für das aktuelle System
cd app/src-tauri && cargo tauri build

# Nur die CLI
cargo build --release -p llm-bridge-cli     # → target/release/llm-bridge
```

Installer für alle drei Plattformen baut der GitHub-Workflow `.github/workflows/release.yml` (bei einem Tag `v*` oder manuell).

Die Oberfläche lässt sich ohne Tauri im Browser ansehen (`app/ui/index.html` öffnen) – sie läuft dann mit Demo-Daten.

## Einrichtung

1. App starten. Beim ersten Start wird eine Konfiguration mit dem Upstream **LangDock EU** und dem Alias `haiku` angelegt.
2. Unter **Upstreams** den LangDock-API-Schlüssel eintragen und „Schlüssel speichern“.
3. „Modelle abrufen“ klicken. Die Liste zeigt die tatsächlichen Modell-IDs eures Workspaces. Ein Klick auf eine ID legt eine Zuordnung an. **Die voreingestellte ID `claude-haiku-5-5-default` ist eine Annahme und sollte hier überprüft werden.**
4. Unter **Modelle** „Testen“ klicken (sendet 1 Token).
5. In MacWhisper einen OpenAI-kompatiblen Anbieter anlegen:

| Feld | Wert |
|---|---|
| Basis-URL | `http://127.0.0.1:4000/v1` |
| API-Schlüssel | das lokale Zugriffstoken (`lb-…`, wird beim ersten Start erzeugt; Einstellungen → Kopieren). Ist das Token leer, geht jeder Wert (z. B. `sk-local`) |
| Modellname | der Alias, z. B. `haiku` |

Statt eines Alias akzeptiert der Proxy auch `upstream-id/modell-id`, z. B. `langdock-eu/claude-sonnet-4-6-default`.

## Konfiguration

Gespeichert als TOML (Pfad unter *Einstellungen → Über* bzw. `llm-bridge info`):

| System | Konfiguration |
|---|---|
| macOS | `~/Library/Application Support/ai.it-guy.LLM-Bridge/config.toml` |
| Windows | `%APPDATA%\it-guy\LLM Bridge\config\config.toml` |
| Linux | `~/.config/llmbridge/config.toml` |

Beispiel: [`config.example.toml`](config.example.toml). Überschreibbar mit `LLM_BRIDGE_CONFIG` und `LLM_BRIDGE_DB`.

API-Schlüssel stehen nie in der Datei. Reihenfolge der Suche: Umgebungsvariable aus `api_key_env` → `LLM_BRIDGE_KEY_<ID>` (z. B. `LLM_BRIDGE_KEY_LANGDOCK_EU`) → Schlüsselbund.

## CLI

```bash
llm-bridge serve [--port 4000]      # Proxy im Vordergrund
llm-bridge set-key langdock-eu      # Schlüssel verdeckt eingeben → Schlüsselbund
llm-bridge models langdock-eu       # Modell-IDs beim Upstream abfragen
llm-bridge test haiku               # Verbindungstest
llm-bridge stats --range 30d        # Statistik (24h, 7d, 30d, 90d, all; --json)
llm-bridge export stats.csv         # CSV-Export
llm-bridge info                     # Pfade, Upstreams, Schlüsselquelle
```

Beispiel für einen Linux-Server mit systemd:

```ini
[Service]
Environment=LLM_BRIDGE_KEY_LANGDOCK_EU=sk-…
ExecStart=/usr/local/bin/llm-bridge serve
Restart=on-failure
```

## Statistik

Pro Anfrage werden gespeichert: Zeitpunkt, Client, Alias, Upstream, Modell-ID, Input-, Output- und Cache-Tokens, Dauer, HTTP-Status, Streaming ja/nein, Fehlermeldung. **Prompt- und Antwortinhalte werden nicht gespeichert.** Bricht ein Client einen Stream ab, wird die Anfrage mit den bis dahin gezählten Tokens und Status 499 erfasst.

Die Input-Tokens in der Oberfläche enthalten Cache-Lese- und -Schreib-Tokens, so wie OpenAI `prompt_tokens` zählt.

## Weitere Sprache hinzufügen

1. `app/ui/locales/en.js` nach z. B. `fr.js` kopieren, `.en` durch `.fr` ersetzen und Texte übersetzen (`_name: "Français"`).
2. In `app/ui/index.html` ein `<script src="locales/fr.js"></script>` ergänzen.
3. Optional die Tray-Texte in `app/src-tauri/src/tray.rs` (`fn texts`) erweitern.

Die Sprache erscheint danach automatisch in den Einstellungen.

## Sicherheit

- Der Server bindet standardmäßig nur an `127.0.0.1`.
- Beim ersten Start wird ein zufälliges lokales Zugriffstoken erzeugt. Wer es in den Einstellungen leert, öffnet den Proxy für jedes Programm auf dem Rechner.
- Anfragen aus dem Browser werden abgewiesen: Ein `Origin`-Header muss auf `localhost`/`127.0.0.1`/`[::1]` zeigen, und solange der Server nur lokal lauscht, gilt dasselbe für den `Host`-Header (Schutz vor DNS-Rebinding). `/v1/chat/completions` verlangt `Content-Type: application/json`.
- Bei Bindung an andere Adressen sollte ein lokales Zugriffstoken gesetzt werden (die Oberfläche warnt).
- Inhalte werden nicht protokolliert; Logs enthalten nur Status und Fehlermeldungen.
- Upstreams müssen `https://` verwenden; `http://` ist nur für `localhost` erlaubt. Weiterleitungen des Upstreams werden nicht verfolgt, damit der API-Schlüssel nicht an ein fremdes Ziel geht – stattdessen meldet der Proxy die neue Adresse.
- Konfiguration und Statistik-Datenbank sind nur für den eigenen Benutzer lesbar (0600, Verzeichnis 0700).

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```

## Lizenz

MIT © Martin Schmid
