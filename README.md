# LLM Bridge

> **Language / Sprache**: [DE](#deutsche-dokumentation) | [EN](#english-documentation)

---

## Deutsche Dokumentation

### Überblick

Lokaler Mini-Proxy für macOS, Windows und Linux, der **OpenAI-Chat-Completions** in **Anthropic-Messages** übersetzt. Damit können Tools, die nur „OpenAI-kompatible“ Anbieter kennen – etwa MacWhisper –, Claude-Modelle über **LangDock** oder **Anthropic direkt** nutzen. Jede Anfrage wird mit ihrem Token-Verbrauch in einer lokalen Statistik erfasst.

```
MacWhisper ──OpenAI──▶ LLM Bridge (127.0.0.1:4000/v1) ──Anthropic──▶ LangDock /anthropic/eu/v1/messages
                              │
                              └─▶ SQLite-Statistik (nur Metadaten, keine Inhalte)
```

### Funktionen

- OpenAI-Endpunkte `/v1/chat/completions` (mit und ohne Streaming) und `/v1/models`
- Übersetzung von System-Prompts, Bildern (Base64), Tools/Function Calling, `stop`, `temperature`, `max_tokens`, JSON-Modus
- SSE-Streaming: Anthropic-Events → OpenAI-Chunks inkl. `stream_options.include_usage`
- Fehler im OpenAI-Format mit verständlicher Meldung (z. B. falsche Modell-ID)
- **Token-Statistik**: Anfragen, Input/Output/Cache-Tokens, Antwortzeiten – je Stunde/Tag, je Modell, je Client (erkannt am User-Agent), CSV-Export
- API-Schlüssel im **System-Schlüsselbund** (macOS Keychain, Windows Anmeldeinformationsverwaltung, Linux Secret Service), alternativ per Umgebungsvariable
- Tray-App (Tauri 2) mit Start/Stopp, Autostart bei Anmeldung, Live-Aktualisierung
- **Mehrsprachig** (Deutsch, Englisch; automatische Erkennung der Systemsprache)
- Headless-CLI für Server und Skripte – teilt Konfiguration und Statistik mit der App

### Installation

Fertige Pakete stehen unter [Releases](https://github.com/eqms/llm-bridge/releases/latest):

| System | Datei |
|---|---|
| macOS mit Apple Silicon | `LLM-Bridge_<Version>_aarch64.dmg` |
| macOS mit Intel | `LLM-Bridge_<Version>_x64.dmg` |
| Windows | `LLM-Bridge_<Version>_x64-setup.exe` (alternativ `.msi`) |
| Linux | `.AppImage`, `.deb` oder `.rpm` |

**macOS**: DMG öffnen, *LLM Bridge* in den Ordner *Programme* ziehen und starten. Die App ist mit
einer Developer ID signiert und von Apple notarisiert, sie startet ohne Warnung. Sie erscheint als
Symbol in der Menüleiste.

**Windows**: Den Installer ausführen. Er ist nicht signiert, deshalb meldet Windows „Der Computer
wurde durch Windows geschützt“. Mit *Weitere Informationen → Trotzdem ausführen* geht es weiter.

**Linux**:

```bash
sudo apt install ./LLM-Bridge_*_amd64.deb                   # Debian, Ubuntu
sudo dnf install ./LLM-Bridge-*.x86_64.rpm                  # Fedora, RHEL
chmod +x LLM-Bridge_*.AppImage && ./LLM-Bridge_*.AppImage   # AppImage (benötigt libfuse2)
```

Für das Tray-Symbol braucht der Desktop AppIndicator-Unterstützung. Unter GNOME ohne Ubuntu-Anpassungen
ist dafür die Erweiterung „AppIndicator and KStatusNotifierItem Support“ nötig.

**Nur die CLI** (z. B. für Server): Die Programmdatei für das eigene System herunterladen, ausführbar
machen und in den Pfad legen. Verfügbar sind `llm-bridge-aarch64-apple-darwin`,
`llm-bridge-x86_64-apple-darwin`, `llm-bridge-x86_64-unknown-linux-gnu` und
`llm-bridge-x86_64-pc-windows-msvc.exe`.

```bash
curl -fLo llm-bridge https://github.com/eqms/llm-bridge/releases/latest/download/llm-bridge-x86_64-unknown-linux-gnu
chmod +x llm-bridge && sudo mv llm-bridge /usr/local/bin/
llm-bridge --version
```

**Prüfsummen**: Jedes Release enthält `SHA256SUMS`. Im Download-Ordner prüfen mit
`shasum -a 256 -c --ignore-missing SHA256SUMS` (macOS) bzw. `sha256sum -c --ignore-missing SHA256SUMS` (Linux).

Danach aktualisieren sich App und CLI selbst, siehe [Updates](#updates). Liegt die CLI in einem
Systemverzeichnis wie `/usr/local/bin`, braucht `llm-bridge update` dort Schreibrechte (`sudo`).

### Einrichtung

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

### Konfiguration

Gespeichert als TOML (Pfad unter *Einstellungen → Über* bzw. `llm-bridge info`):

| System | Konfiguration |
|---|---|
| macOS | `~/Library/Application Support/ai.it-guy.LLM-Bridge/config.toml` |
| Windows | `%APPDATA%\it-guy\LLM Bridge\config\config.toml` |
| Linux | `~/.config/llmbridge/config.toml` |

Beispiel: [`config.example.toml`](config.example.toml). Überschreibbar mit `LLM_BRIDGE_CONFIG` und `LLM_BRIDGE_DB`.

API-Schlüssel stehen nie in der Datei. Reihenfolge der Suche: Umgebungsvariable aus `api_key_env` → `LLM_BRIDGE_KEY_<ID>` (z. B. `LLM_BRIDGE_KEY_LANGDOCK_EU`) → Schlüsselbund.

### CLI

```bash
llm-bridge serve [--host H] [--port 4000]   # Proxy im Vordergrund
llm-bridge set-key langdock-eu      # Schlüssel verdeckt eingeben → Schlüsselbund
llm-bridge delete-key langdock-eu   # Schlüssel entfernen
llm-bridge models langdock-eu       # Modell-IDs beim Upstream abfragen
llm-bridge test haiku               # Verbindungstest
llm-bridge stats --range 30d        # Statistik (24h, 7d, 30d, 90d, all; --json)
llm-bridge export stats.csv         # CSV-Export
llm-bridge reset-stats [--yes]      # Statistik löschen
llm-bridge info                     # Pfade, Upstreams, Schlüsselquelle
llm-bridge update [--check]         # auf die neueste Version aktualisieren
```

Beispiel für einen Linux-Server mit systemd:

```ini
[Service]
Environment=LLM_BRIDGE_KEY_LANGDOCK_EU=sk-…
ExecStart=/usr/local/bin/llm-bridge serve
Restart=on-failure
```

### Updates

Die App sucht kurz nach dem Start und danach alle sechs Stunden nach einer neuen Version. Ein Update
wird im Hintergrund geladen und erst installiert, wenn der Proxy fünf Minuten lang keine
Chat-Anfrage bearbeitet hat. Bis dahin zeigt das Tray-Menü das wartende Update an. Danach startet
die App neu, und der Proxy läuft wieder. Abschalten lässt sich das unter *Einstellungen →
Automatisch aktualisieren*.

Die CLI aktualisiert sich mit `llm-bridge update`. Ein laufendes `llm-bridge serve` muss danach
neu gestartet werden, unter systemd etwa mit `systemctl restart`.

Jedes Update ist mit einem eigenen Schlüssel signiert (minisign). App und CLI installieren nur,
wenn die Signatur passt und für genau die angekündigte Version erstellt wurde.

### Statistik

Pro Anfrage werden gespeichert: Zeitpunkt, Client, Alias, Upstream, Modell-ID, Input-, Output- und Cache-Tokens, Dauer, HTTP-Status, Streaming ja/nein, Fehlermeldung. **Prompt- und Antwortinhalte werden nicht gespeichert.** Bricht ein Client einen Stream ab, wird die Anfrage mit den bis dahin gezählten Tokens und Status 499 erfasst.

Die Input-Tokens in der Oberfläche enthalten Cache-Lese- und -Schreib-Tokens, so wie OpenAI `prompt_tokens` zählt.

### Weitere Sprache hinzufügen

1. `app/ui/locales/en.js` nach z. B. `fr.js` kopieren, `.en` durch `.fr` ersetzen und Texte übersetzen (`_name: "Français"`).
2. In `app/ui/index.html` ein `<script src="locales/fr.js"></script>` ergänzen.
3. Optional die Tray-Texte in `app/src-tauri/src/tray.rs` (`fn texts`) erweitern.

Die Sprache erscheint danach automatisch in den Einstellungen.

### Sicherheit

- Der Server bindet standardmäßig nur an `127.0.0.1`.
- Beim ersten Start wird ein zufälliges lokales Zugriffstoken erzeugt. Wer es in den Einstellungen leert, öffnet den Proxy für jedes Programm auf dem Rechner.
- Anfragen aus dem Browser werden abgewiesen: Ein `Origin`-Header muss auf `localhost`/`127.0.0.1`/`[::1]` zeigen, und solange der Server nur lokal lauscht, gilt dasselbe für den `Host`-Header (Schutz vor DNS-Rebinding). `/v1/chat/completions` verlangt `Content-Type: application/json`.
- Bei Bindung an andere Adressen sollte ein lokales Zugriffstoken gesetzt werden (die Oberfläche warnt).
- Inhalte werden nicht protokolliert; Logs enthalten nur Status und Fehlermeldungen.
- Upstreams müssen `https://` verwenden; `http://` ist nur für `localhost` erlaubt. Weiterleitungen des Upstreams werden nicht verfolgt, damit der API-Schlüssel nicht an ein fremdes Ziel geht – stattdessen meldet der Proxy die neue Adresse.
- Konfiguration und Statistik-Datenbank sind nur für den eigenen Benutzer lesbar (0600, Verzeichnis 0700).

### Projektaufbau

| Pfad | Inhalt |
|---|---|
| `crates/core` | Proxy-Logik als Library: Konfiguration, Übersetzung, Streaming, Statistik, axum-Server |
| `crates/cli` | Binary `llm-bridge` ohne GUI |
| `app/src-tauri` | Tauri-Backend: Tray, Fenster, Befehle für die Oberfläche |
| `app/ui` | Oberfläche (HTML/CSS/JS ohne Build-Schritt), Sprachdateien in `app/ui/locales/` |

### Aus dem Quellcode bauen

Voraussetzungen: Rust ≥ 1.90 und die [Tauri-Systemabhängigkeiten](https://v2.tauri.app/start/prerequisites/). Unter Linux z. B.:

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

### Tests

```bash
cargo fmt --check                                  # rustfmt.toml: max_width 120
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p llm-bridge-core -- --ignored         # schreibt in den echten Schlüsselbund
cargo deny check                                   # Advisories, Lizenzen, Quellen (deny.toml)
```

### Release

Ein Tag `v<Version>` (passend zu `version` in `Cargo.toml`) startet `.github/workflows/release.yml`:
Prüfungen, Build für macOS (Apple Silicon und Intel), Windows und Linux, danach ein **Release-Entwurf**
mit allen Paketen, `SHA256SUMS` und dem Update-Manifest `latest.json`. Veröffentlicht wird der
Entwurf von Hand; erst dann sehen installierte Apps und `llm-bridge update` die neue Version.

- **macOS**: App, DMG und CLI sind mit einer Developer ID signiert und von Apple notarisiert.
- **Windows und Linux**: unsigniert; die Prüfsummen stehen in `SHA256SUMS`.

Benötigte Repository-Secrets:

| Secret | Inhalt |
|---|---|
| `APPLE_CERTIFICATE` | Developer-ID-Application-Zertifikat als `.p12`, Base64-kodiert |
| `APPLE_CERTIFICATE_PASSWORD` | Passwort der `.p12`-Datei |
| `APPLE_API_KEY` | Key-ID des App-Store-Connect-API-Schlüssels (Notarisierung) |
| `APPLE_API_ISSUER` | Issuer-ID aus App Store Connect |
| `APPLE_API_KEY_P8` | Inhalt der Datei `AuthKey_<Key-ID>.p8` |
| `TAURI_SIGNING_PRIVATE_KEY` | Privater Update-Schlüssel (Inhalt der Datei aus `cargo tauri signer generate`) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Passwort dieses Schlüssels |

Der öffentliche Update-Schlüssel steht in `app/src-tauri/tauri.conf.json` und
`crates/core/src/update.rs`; ein Test prüft, dass beide übereinstimmen. Geht der private
Schlüssel verloren, können bestehende Installationen keine Updates mehr annehmen.

### Lizenz

MIT © Martin Schmid

---

## English Documentation

### Overview

A small local proxy for macOS, Windows and Linux that translates **OpenAI Chat Completions** into **Anthropic Messages**. It lets tools that only know “OpenAI-compatible” providers – such as MacWhisper – use Claude models through **LangDock** or **Anthropic directly**. Every request is recorded in a local statistics database together with its token usage.

```
MacWhisper ──OpenAI──▶ LLM Bridge (127.0.0.1:4000/v1) ──Anthropic──▶ LangDock /anthropic/eu/v1/messages
                              │
                              └─▶ SQLite statistics (metadata only, no content)
```

### Features

- OpenAI endpoints `/v1/chat/completions` (streaming and non-streaming) and `/v1/models`
- Translation of system prompts, images (Base64), tools/function calling, `stop`, `temperature`, `max_tokens`, JSON mode
- SSE streaming: Anthropic events → OpenAI chunks, including `stream_options.include_usage`
- Errors in OpenAI format with a clear message (e.g. wrong model ID)
- **Token statistics**: requests, input/output/cache tokens, response times – per hour/day, per model, per client (detected from the User-Agent), CSV export
- API keys in the **system keychain** (macOS Keychain, Windows Credential Manager, Linux Secret Service), or via environment variable
- Tray app (Tauri 2) with start/stop, autostart at login, live updates
- **Multilingual** (German, English; the system language is detected automatically)
- Headless CLI for servers and scripts – shares configuration and statistics with the app

### Installation

Ready-made packages are available under [Releases](https://github.com/eqms/llm-bridge/releases/latest):

| System | File |
|---|---|
| macOS on Apple Silicon | `LLM-Bridge_<version>_aarch64.dmg` |
| macOS on Intel | `LLM-Bridge_<version>_x64.dmg` |
| Windows | `LLM-Bridge_<version>_x64-setup.exe` (or `.msi`) |
| Linux | `.AppImage`, `.deb` or `.rpm` |

**macOS**: Open the DMG, drag *LLM Bridge* into *Applications* and launch it. The app is signed with a
Developer ID and notarized by Apple, so it opens without a warning. It shows up as an icon in the
menu bar.

**Windows**: Run the installer. It is not signed, so Windows shows "Windows protected your PC".
Choose *More info → Run anyway* to continue.

**Linux**:

```bash
sudo apt install ./LLM-Bridge_*_amd64.deb                   # Debian, Ubuntu
sudo dnf install ./LLM-Bridge-*.x86_64.rpm                  # Fedora, RHEL
chmod +x LLM-Bridge_*.AppImage && ./LLM-Bridge_*.AppImage   # AppImage (needs libfuse2)
```

The tray icon needs AppIndicator support in the desktop. On GNOME without Ubuntu's tweaks, install
the "AppIndicator and KStatusNotifierItem Support" extension.

**CLI only** (e.g. for servers): Download the binary for your system, make it executable and put it
on the path. Available are `llm-bridge-aarch64-apple-darwin`, `llm-bridge-x86_64-apple-darwin`,
`llm-bridge-x86_64-unknown-linux-gnu` and `llm-bridge-x86_64-pc-windows-msvc.exe`.

```bash
curl -fLo llm-bridge https://github.com/eqms/llm-bridge/releases/latest/download/llm-bridge-x86_64-unknown-linux-gnu
chmod +x llm-bridge && sudo mv llm-bridge /usr/local/bin/
llm-bridge --version
```

**Checksums**: Every release ships `SHA256SUMS`. Verify in the download folder with
`shasum -a 256 -c --ignore-missing SHA256SUMS` (macOS) or `sha256sum -c --ignore-missing SHA256SUMS` (Linux).

From then on, app and CLI update themselves, see [Updates](#updates-1). If the CLI lives in a system
directory such as `/usr/local/bin`, `llm-bridge update` needs write access there (`sudo`).

### Setup

1. Start the app. On first launch it creates a configuration with the **LangDock EU** upstream and the alias `haiku`.
2. Under **Upstreams**, enter your LangDock API key and click “Save key”.
3. Click “Fetch models”. The list shows the actual model IDs of your workspace. Clicking an ID creates a mapping. **The preset ID `claude-haiku-5-5-default` is an assumption and should be verified here.**
4. Under **Models**, click “Test” (sends 1 token).
5. In MacWhisper, add an OpenAI-compatible provider:

| Field | Value |
|---|---|
| Base URL | `http://127.0.0.1:4000/v1` |
| API key | the local access token (`lb-…`, generated on first launch; Settings → Copy). If the token is empty, any value works (e.g. `sk-local`) |
| Model name | the alias, e.g. `haiku` |

Instead of an alias, the proxy also accepts `upstream-id/model-id`, e.g. `langdock-eu/claude-sonnet-4-6-default`.

### Configuration

Stored as TOML (path shown under *Settings → About* or via `llm-bridge info`):

| System | Configuration |
|---|---|
| macOS | `~/Library/Application Support/ai.it-guy.LLM-Bridge/config.toml` |
| Windows | `%APPDATA%\it-guy\LLM Bridge\config\config.toml` |
| Linux | `~/.config/llmbridge/config.toml` |

Example: [`config.example.toml`](config.example.toml). Can be overridden with `LLM_BRIDGE_CONFIG` and `LLM_BRIDGE_DB`.

API keys are never stored in the file. Lookup order: environment variable named in `api_key_env` → `LLM_BRIDGE_KEY_<ID>` (e.g. `LLM_BRIDGE_KEY_LANGDOCK_EU`) → keychain.

### CLI

```bash
llm-bridge serve [--host H] [--port 4000]   # run the proxy in the foreground
llm-bridge set-key langdock-eu      # enter the key hidden → keychain
llm-bridge delete-key langdock-eu   # remove the key
llm-bridge models langdock-eu       # list model IDs from the upstream
llm-bridge test haiku               # connection test
llm-bridge stats --range 30d        # statistics (24h, 7d, 30d, 90d, all; --json)
llm-bridge export stats.csv         # CSV export
llm-bridge reset-stats [--yes]      # delete statistics
llm-bridge info                     # paths, upstreams, key source
llm-bridge update [--check]         # update to the latest release
```

Example for a Linux server with systemd:

```ini
[Service]
Environment=LLM_BRIDGE_KEY_LANGDOCK_EU=sk-…
ExecStart=/usr/local/bin/llm-bridge serve
Restart=on-failure
```

### Updates

The app checks for a new version shortly after start and then every six hours. An update is
downloaded in the background and installed only once the proxy has not handled a chat request for
five minutes. Until then the tray menu shows the pending update. The app then restarts and the
proxy is back up. Turn this off under *Settings → Update automatically*.

The CLI updates itself with `llm-bridge update`. A running `llm-bridge serve` must be restarted
afterwards, for example with `systemctl restart` under systemd.

Every update is signed with a dedicated key (minisign). App and CLI only install it if the
signature is valid and was made for exactly the announced version.

### Statistics

For each request the following is stored: timestamp, client, alias, upstream, model ID, input, output and cache tokens, duration, HTTP status, streaming yes/no, error message. **Prompt and response contents are not stored.** If a client aborts a stream, the request is recorded with the tokens counted so far and status 499.

Input tokens in the UI include cache read and write tokens, matching how OpenAI counts `prompt_tokens`.

### Adding Another Language

1. Copy `app/ui/locales/en.js` to e.g. `fr.js`, replace `.en` with `.fr` and translate the texts (`_name: "Français"`).
2. Add a `<script src="locales/fr.js"></script>` to `app/ui/index.html`.
3. Optionally extend the tray texts in `app/src-tauri/src/tray.rs` (`fn texts`).

The language then appears in the settings automatically.

### Security

- By default the server binds to `127.0.0.1` only.
- A random local access token is generated on first launch. Clearing it in the settings opens the proxy to every program on the machine.
- Requests from browsers are rejected: an `Origin` header must point to `localhost`/`127.0.0.1`/`[::1]`, and as long as the server listens locally only, the same applies to the `Host` header (protection against DNS rebinding). `/v1/chat/completions` requires `Content-Type: application/json`.
- When binding to other addresses, a local access token should be set (the UI warns about this).
- Contents are not logged; logs contain only status and error messages.
- Upstreams must use `https://`; `http://` is allowed for `localhost` only. Redirects from the upstream are not followed so the API key cannot be sent to a foreign target – the proxy reports the new address instead.
- Configuration and statistics database are readable by the owning user only (0600, directory 0700).

### Project Layout

| Path | Contents |
|---|---|
| `crates/core` | Proxy logic as a library: configuration, translation, streaming, statistics, axum server |
| `crates/cli` | The `llm-bridge` binary without a GUI |
| `app/src-tauri` | Tauri backend: tray, windows, commands for the UI |
| `app/ui` | User interface (HTML/CSS/JS, no build step), language files in `app/ui/locales/` |

### Building from Source

Requirements: Rust ≥ 1.90 and the [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/). On Linux, for example:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

```bash
cargo install tauri-cli --version "^2" --locked

# Run the app in development mode
cd app/src-tauri && cargo tauri dev

# Installers (.dmg/.app, .msi/.exe, .deb/.AppImage/.rpm) for the current system
cd app/src-tauri && cargo tauri build

# CLI only
cargo build --release -p llm-bridge-cli     # → target/release/llm-bridge
```

Installers for all three platforms are built by the GitHub workflow `.github/workflows/release.yml` (on a `v*` tag or manually).

The UI can be previewed in a browser without Tauri (open `app/ui/index.html`); it then runs with demo data.

### Tests

```bash
cargo fmt --check                                  # rustfmt.toml: max_width 120
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p llm-bridge-core -- --ignored         # writes to the real keychain
cargo deny check                                   # advisories, licenses, sources (deny.toml)
```

### Release

A tag `v<version>` (matching `version` in `Cargo.toml`) triggers `.github/workflows/release.yml`:
checks, builds for macOS (Apple Silicon and Intel), Windows and Linux, then a **draft release**
with all packages, `SHA256SUMS` and the update manifest `latest.json`. The draft is published
manually; only then do installed apps and `llm-bridge update` see the new version.

- **macOS**: app, DMG and CLI are signed with a Developer ID and notarized by Apple.
- **Windows and Linux**: unsigned; checksums are listed in `SHA256SUMS`.

Required repository secrets:

| Secret | Contents |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application certificate as `.p12`, Base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | Password of the `.p12` file |
| `APPLE_API_KEY` | Key ID of the App Store Connect API key (notarization) |
| `APPLE_API_ISSUER` | Issuer ID from App Store Connect |
| `APPLE_API_KEY_P8` | Contents of the file `AuthKey_<Key-ID>.p8` |
| `TAURI_SIGNING_PRIVATE_KEY` | Private update key (contents of the file from `cargo tauri signer generate`) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password of that key |

The public update key lives in `app/src-tauri/tauri.conf.json` and `crates/core/src/update.rs`;
a test checks that both match. If the private key is lost, existing installations can no longer
accept updates.

### License

MIT © Martin Schmid
