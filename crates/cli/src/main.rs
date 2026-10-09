use clap::{Parser, Subcommand};
use llm_bridge_core::{config, secrets, stats::Range, Config, ProxyState, Stats};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "llm-bridge",
    version,
    about = "OpenAI-kompatibler Mini-Proxy für Anthropic-Endpunkte (z. B. LangDock)"
)]
struct Cli {
    /// Pfad zur Konfiguration (Standard: Nutzer-Konfigurationsverzeichnis)
    #[arg(long, global = true, env = "LLM_BRIDGE_CONFIG")]
    config: Option<PathBuf>,
    /// Pfad zur Statistik-Datenbank
    #[arg(long, global = true, env = "LLM_BRIDGE_DB")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Proxy starten (Vordergrund, Strg+C beendet)
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Token-Statistik anzeigen
    Stats {
        /// 24h, 7d, 30d, 90d, all
        #[arg(long, default_value = "7d")]
        range: String,
        /// Ausgabe als JSON
        #[arg(long)]
        json: bool,
    },
    /// Statistik als CSV exportieren
    Export {
        path: PathBuf,
        #[arg(long, default_value = "all")]
        range: String,
    },
    /// Statistik löschen
    ResetStats {
        #[arg(long)]
        yes: bool,
    },
    /// API-Schlüssel für einen Upstream im Schlüsselbund speichern (Eingabe verdeckt)
    SetKey { upstream: String },
    /// API-Schlüssel entfernen
    DeleteKey { upstream: String },
    /// Verfügbare Modelle beim Upstream abfragen
    Models { upstream: String },
    /// Verbindung über einen Alias testen (verbraucht 1 Output-Token)
    Test { model: String },
    /// Pfade und Konfiguration anzeigen
    Info,
}

fn fmt_n(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hyper=warn,reqwest=warn".into()),
        )
        .with_target(false)
        .init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let config_path = cli.config.clone().unwrap_or_else(config::default_config_path);
    let db_path = cli.db.clone().unwrap_or_else(config::default_db_path);
    let mut cfg = Config::load_or_init(&config_path)?;
    let stats = || Stats::open(&db_path);
    let parse_range =
        |r: &str| Range::parse(r).ok_or_else(|| format!("Unbekannter Zeitraum '{r}' (24h, 7d, 30d, 90d, all)"));

    match cli.cmd {
        Cmd::Serve { host, port } => {
            if let Some(h) = host {
                cfg.server.host = h;
            }
            if let Some(p) = port {
                cfg.server.port = p;
            }
            let state = ProxyState::new(cfg.clone(), stats()?);
            let handle = llm_bridge_core::start(state).await?;
            println!("LLM Bridge {} läuft auf http://{}/v1", llm_bridge_core::VERSION, handle.addr);
            for m in &cfg.models {
                println!("  {:<16} → {} / {}", m.alias, m.upstream, m.model);
            }
            tokio::signal::ctrl_c().await.map_err(|e| e.to_string())?;
            println!("Beende …");
            handle.stop().await;
        }
        Cmd::Stats { range: range_arg, json } => {
            let range = parse_range(&range_arg)?;
            let report = stats()?.report(range, 10)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
                return Ok(());
            }
            let s = &report.summary;
            println!("Zeitraum:        {range_arg}");
            println!("Anfragen:        {} ({} Fehler)", fmt_n(s.requests), fmt_n(s.errors));
            println!("Input-Tokens:    {}", fmt_n(s.input_tokens));
            println!("Output-Tokens:   {}", fmt_n(s.output_tokens));
            println!("Cache lesen:     {}", fmt_n(s.cache_read_tokens));
            println!("Cache schreiben: {}", fmt_n(s.cache_write_tokens));
            println!("Gesamt:          {}", fmt_n(s.total_tokens));
            println!("Ø Dauer:         {} ms", s.avg_duration_ms);
            if !report.by_model.is_empty() {
                println!("\nNach Modell:");
                for g in &report.by_model {
                    println!(
                        "  {:<24} {:>6} Anfr.  {:>12} in  {:>12} out",
                        g.key,
                        g.requests,
                        fmt_n(g.input_tokens),
                        fmt_n(g.output_tokens)
                    );
                }
            }
            if !report.by_client.is_empty() {
                println!("\nNach Client:");
                for g in &report.by_client {
                    println!(
                        "  {:<24} {:>6} Anfr.  {:>12} in  {:>12} out",
                        g.key,
                        g.requests,
                        fmt_n(g.input_tokens),
                        fmt_n(g.output_tokens)
                    );
                }
            }
        }
        Cmd::Export { path, range } => {
            let n = stats()?.export_csv(parse_range(&range)?, &path)?;
            println!("{n} Einträge nach {} exportiert", path.display());
        }
        Cmd::ResetStats { yes } => {
            if !yes {
                return Err("Zum Bestätigen --yes angeben".into());
            }
            stats()?.reset()?;
            println!("Statistik gelöscht");
        }
        Cmd::SetKey { upstream } => {
            if cfg.upstream(&upstream).is_none() {
                return Err(format!("Unbekannter Upstream '{upstream}'"));
            }
            let key =
                rpassword::prompt_password(format!("API-Schlüssel für {upstream}: ")).map_err(|e| e.to_string())?;
            secrets::set_api_key(&upstream, &key)?;
            println!("Gespeichert.");
        }
        Cmd::DeleteKey { upstream } => {
            secrets::delete_api_key(&upstream)?;
            println!("Entfernt.");
        }
        Cmd::Models { upstream } => {
            let state = ProxyState::new(cfg, Stats::in_memory()?);
            for id in state.upstream_models(&upstream).await? {
                println!("{id}");
            }
        }
        Cmd::Test { model } => {
            let state = ProxyState::new(cfg, stats()?);
            let r = state.test_route(&model).await;
            println!(
                "{} – HTTP {} in {} ms: {}",
                if r.ok { "OK" } else { "FEHLER" },
                r.status,
                r.latency_ms,
                r.message
            );
            if !r.ok {
                return Err("Test fehlgeschlagen".into());
            }
        }
        Cmd::Info => {
            println!("Version:     {}", llm_bridge_core::VERSION);
            println!("Konfig:      {}", config_path.display());
            println!("Statistik:   {}", db_path.display());
            println!("Endpunkt:    http://{}:{}/v1", cfg.server.host, cfg.server.port);
            for u in &cfg.upstreams {
                println!(
                    "Upstream:    {} ({}) → {}  [Schlüssel: {:?}]",
                    u.id,
                    u.name,
                    u.messages_url(),
                    secrets::key_source(u)
                );
            }
        }
    }
    Ok(())
}
