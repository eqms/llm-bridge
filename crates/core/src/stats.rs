//! Token-Statistik in einer lokalen SQLite-Datenbank.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Ein protokollierter Request. Es werden nur Metadaten gespeichert – keine Inhalte.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestRecord {
    #[serde(default)]
    pub id: i64,
    /// Unix-Zeit in Millisekunden
    pub ts: i64,
    pub client: String,
    pub alias: String,
    pub upstream: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub duration_ms: u64,
    pub status: u16,
    pub stream: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Summary {
    pub requests: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub avg_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRow {
    pub key: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    /// `YYYY-MM-DD` oder `YYYY-MM-DD HH:00` (lokale Zeit)
    pub label: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Auswertungszeitraum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Range {
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
    #[serde(rename = "90d")]
    Quarter,
    #[serde(rename = "all")]
    All,
}

impl Range {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "24h" | "1d" | "day" => Some(Range::Day),
            "7d" | "week" => Some(Range::Week),
            "30d" | "month" => Some(Range::Month),
            "90d" | "quarter" => Some(Range::Quarter),
            "all" => Some(Range::All),
            _ => None,
        }
    }

    fn since_ms(self) -> i64 {
        let day = 86_400_000i64;
        let now = now_ms();
        match self {
            Range::Day => now - day,
            Range::Week => now - 7 * day,
            Range::Month => now - 30 * day,
            Range::Quarter => now - 90 * day,
            Range::All => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub range: Range,
    pub summary: Summary,
    pub series: Vec<Bucket>,
    pub by_model: Vec<GroupRow>,
    pub by_client: Vec<GroupRow>,
    pub recent: Vec<RequestRecord>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Clone)]
pub struct Stats {
    conn: Arc<Mutex<Connection>>,
    tx: broadcast::Sender<RequestRecord>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS requests (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    ts                 INTEGER NOT NULL,
    client             TEXT    NOT NULL DEFAULT '',
    alias              TEXT    NOT NULL DEFAULT '',
    upstream           TEXT    NOT NULL DEFAULT '',
    model              TEXT    NOT NULL DEFAULT '',
    input_tokens       INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    duration_ms        INTEGER NOT NULL DEFAULT 0,
    status             INTEGER NOT NULL DEFAULT 0,
    stream             INTEGER NOT NULL DEFAULT 0,
    error              TEXT
);
CREATE INDEX IF NOT EXISTS idx_requests_ts ON requests(ts);
"#;

impl Stats {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            crate::config::create_private_dir(dir)?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let stats = Self::init(conn)?;
        // Enthält Client-Namen und Fehlertexte. SQLite legt -wal/-shm mit den Rechten der DB an;
        // bereits vorhandene Dateien werden hier mit korrigiert.
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.as_os_str().to_owned();
            p.push(suffix);
            let p = std::path::PathBuf::from(p);
            if p.exists() {
                crate::config::make_private(&p)?;
            }
        }
        Ok(stats)
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::init(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn init(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        let (tx, _) = broadcast::channel(64);
        Ok(Self { conn: Arc::new(Mutex::new(conn)), tx })
    }

    /// Live-Benachrichtigung über neue Einträge (für die GUI).
    pub fn subscribe(&self) -> broadcast::Receiver<RequestRecord> {
        self.tx.subscribe()
    }

    /// Synchrones Speichern – ein Insert dauert Mikrosekunden.
    pub fn record(&self, mut r: RequestRecord) {
        let result = {
            let conn = match self.conn.lock() {
                Ok(c) => c,
                Err(p) => p.into_inner(),
            };
            conn.execute(
                "INSERT INTO requests (ts, client, alias, upstream, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, duration_ms, status, stream, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    r.ts,
                    r.client,
                    r.alias,
                    r.upstream,
                    r.model,
                    r.input_tokens as i64,
                    r.output_tokens as i64,
                    r.cache_read_tokens as i64,
                    r.cache_write_tokens as i64,
                    r.duration_ms as i64,
                    r.status as i64,
                    r.stream as i64,
                    r.error
                ],
            )
            .map(|_| conn.last_insert_rowid())
        };
        match result {
            Ok(id) => {
                r.id = id;
                let _ = self.tx.send(r);
            }
            Err(e) => tracing::warn!("Statistik konnte nicht gespeichert werden: {e}"),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(p) => p.into_inner(),
        };
        f(&conn).map_err(|e| e.to_string())
    }

    pub fn summary(&self, range: Range) -> Result<Summary, String> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN status >= 400 OR error IS NOT NULL THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0),
                        COALESCE(AVG(duration_ms), 0)
                 FROM requests WHERE ts >= ?1",
                params![range.since_ms()],
                |row| {
                    let input: i64 = row.get(2)?;
                    let output: i64 = row.get(3)?;
                    let cr: i64 = row.get(4)?;
                    let cw: i64 = row.get(5)?;
                    let avg: f64 = row.get(6)?;
                    Ok(Summary {
                        requests: row.get::<_, i64>(0)? as u64,
                        errors: row.get::<_, i64>(1)? as u64,
                        input_tokens: input as u64,
                        output_tokens: output as u64,
                        cache_read_tokens: cr as u64,
                        cache_write_tokens: cw as u64,
                        total_tokens: (input + output + cr + cw) as u64,
                        avg_duration_ms: avg.round() as u64,
                    })
                },
            )
        })
    }

    fn group(&self, column: &str, range: Range) -> Result<Vec<GroupRow>, String> {
        // `column` ist intern festgelegt (kein Nutzereingabewert)
        let sql = format!(
            "SELECT {column}, COUNT(*), COALESCE(SUM(input_tokens + cache_read_tokens + cache_write_tokens), 0),
                    COALESCE(SUM(output_tokens), 0)
             FROM requests WHERE ts >= ?1
             GROUP BY {column} ORDER BY 3 + 4 DESC, 2 DESC LIMIT 50"
        );
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params![range.since_ms()], |row| {
                Ok(GroupRow {
                    key: row.get(0)?,
                    requests: row.get::<_, i64>(1)? as u64,
                    input_tokens: row.get::<_, i64>(2)? as u64,
                    output_tokens: row.get::<_, i64>(3)? as u64,
                })
            })?;
            rows.collect()
        })
    }

    pub fn by_model(&self, range: Range) -> Result<Vec<GroupRow>, String> {
        self.group("alias", range)
    }

    pub fn by_client(&self, range: Range) -> Result<Vec<GroupRow>, String> {
        self.group("client", range)
    }

    /// Zeitreihe mit lückenlos aufgefüllten Buckets (Stunden für 24h, sonst Tage).
    pub fn series(&self, range: Range) -> Result<Vec<Bucket>, String> {
        let (start, step, fmt, end) = match range {
            Range::Day => (
                "strftime('%Y-%m-%d %H:00', 'now', 'localtime', '-23 hours')".to_string(),
                "+1 hour",
                "%Y-%m-%d %H:00",
                "strftime('%Y-%m-%d %H:00', 'now', 'localtime')",
            ),
            Range::Week => {
                ("date('now', 'localtime', '-6 days')".into(), "+1 day", "%Y-%m-%d", "date('now', 'localtime')")
            }
            Range::Month => {
                ("date('now', 'localtime', '-29 days')".into(), "+1 day", "%Y-%m-%d", "date('now', 'localtime')")
            }
            Range::Quarter => {
                ("date('now', 'localtime', '-89 days')".into(), "+1 day", "%Y-%m-%d", "date('now', 'localtime')")
            }
            Range::All => {
                let first: Option<i64> =
                    self.with(|c| c.query_row("SELECT MIN(ts) FROM requests", [], |r| r.get(0)).optional())?.flatten();
                let start = match first {
                    Some(ts) => format!("date({}, 'unixepoch', 'localtime')", ts / 1000),
                    None => "date('now', 'localtime')".into(),
                };
                (start, "+1 day", "%Y-%m-%d", "date('now', 'localtime')")
            }
        };
        let sql = format!(
            "WITH RECURSIVE b(label) AS (
                SELECT {start}
                UNION ALL
                SELECT strftime('{fmt}', label, '{step}') FROM b WHERE label < {end}
             )
             SELECT b.label, COUNT(r.id),
                    COALESCE(SUM(r.input_tokens + r.cache_read_tokens + r.cache_write_tokens), 0),
                    COALESCE(SUM(r.output_tokens), 0)
             FROM b LEFT JOIN requests r
               ON strftime('{fmt}', r.ts / 1000, 'unixepoch', 'localtime') = b.label
             GROUP BY b.label ORDER BY b.label
             LIMIT 1000"
        );
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([], |row| {
                Ok(Bucket {
                    label: row.get(0)?,
                    requests: row.get::<_, i64>(1)? as u64,
                    input_tokens: row.get::<_, i64>(2)? as u64,
                    output_tokens: row.get::<_, i64>(3)? as u64,
                })
            })?;
            rows.collect()
        })
    }

    pub fn recent(&self, limit: u32) -> Result<Vec<RequestRecord>, String> {
        self.query_records("ORDER BY ts DESC, id DESC LIMIT ?1", params![limit as i64])
    }

    fn query_records(&self, tail: &str, p: impl rusqlite::Params) -> Result<Vec<RequestRecord>, String> {
        let sql = format!(
            "SELECT id, ts, client, alias, upstream, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, duration_ms, status, stream, error
             FROM requests {tail}"
        );
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(p, |row| {
                Ok(RequestRecord {
                    id: row.get(0)?,
                    ts: row.get(1)?,
                    client: row.get(2)?,
                    alias: row.get(3)?,
                    upstream: row.get(4)?,
                    model: row.get(5)?,
                    input_tokens: row.get::<_, i64>(6)? as u64,
                    output_tokens: row.get::<_, i64>(7)? as u64,
                    cache_read_tokens: row.get::<_, i64>(8)? as u64,
                    cache_write_tokens: row.get::<_, i64>(9)? as u64,
                    duration_ms: row.get::<_, i64>(10)? as u64,
                    status: row.get::<_, i64>(11)? as u16,
                    stream: row.get::<_, i64>(12)? != 0,
                    error: row.get(13)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn report(&self, range: Range, recent_limit: u32) -> Result<Report, String> {
        Ok(Report {
            range,
            summary: self.summary(range)?,
            series: self.series(range)?,
            by_model: self.by_model(range)?,
            by_client: self.by_client(range)?,
            recent: self.recent(recent_limit)?,
        })
    }

    pub fn reset(&self) -> Result<(), String> {
        self.with(|c| c.execute("DELETE FROM requests", []).map(|_| ()))
    }

    /// Schreibt alle Einträge des Zeitraums als CSV (Semikolon-getrennt, Excel-freundlich).
    pub fn export_csv(&self, range: Range, path: &Path) -> Result<usize, String> {
        let rows = self.query_records("WHERE ts >= ?1 ORDER BY ts", params![range.since_ms()])?;
        let mut out = String::from(
            "id;timestamp_ms;client;alias;upstream;model;input_tokens;output_tokens;cache_read_tokens;cache_write_tokens;duration_ms;status;stream;error\n",
        );
        let esc = |s: &str| {
            if s.contains([';', '"', '\n']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };
        for r in &rows {
            out.push_str(&format!(
                "{};{};{};{};{};{};{};{};{};{};{};{};{};{}\n",
                r.id,
                r.ts,
                esc(&r.client),
                esc(&r.alias),
                esc(&r.upstream),
                esc(&r.model),
                r.input_tokens,
                r.output_tokens,
                r.cache_read_tokens,
                r.cache_write_tokens,
                r.duration_ms,
                r.status,
                r.stream as u8,
                esc(r.error.as_deref().unwrap_or(""))
            ));
        }
        std::fs::write(path, out).map_err(|e| e.to_string())?;
        Ok(rows.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(alias: &str, client: &str, i: u64, o: u64, status: u16) -> RequestRecord {
        RequestRecord {
            ts: now_ms(),
            client: client.into(),
            alias: alias.into(),
            upstream: "u".into(),
            model: "m".into(),
            input_tokens: i,
            output_tokens: o,
            duration_ms: 100,
            status,
            ..Default::default()
        }
    }

    #[test]
    fn aggregates() {
        let s = Stats::in_memory().unwrap();
        let mut rx = s.subscribe();
        s.record(rec("haiku", "MacWhisper", 100, 20, 200));
        s.record(rec("haiku", "MacWhisper", 50, 10, 200));
        s.record(rec("sonnet", "curl", 7, 0, 502));
        assert_eq!(rx.try_recv().unwrap().alias, "haiku");

        let sum = s.summary(Range::Day).unwrap();
        assert_eq!(sum.requests, 3);
        assert_eq!(sum.errors, 1);
        assert_eq!(sum.input_tokens, 157);
        assert_eq!(sum.total_tokens, 187);

        let by_model = s.by_model(Range::Week).unwrap();
        assert_eq!(by_model[0].key, "haiku");
        assert_eq!(by_model[0].requests, 2);

        for (range, n) in [(Range::Day, 24), (Range::Week, 7), (Range::Month, 30), (Range::Quarter, 90)] {
            let series = s.series(range).unwrap();
            assert_eq!(series.len(), n, "{range:?}");
            let total: u64 = series.iter().map(|b| b.requests).sum();
            assert_eq!(total, 3, "{range:?}");
        }
        assert_eq!(s.series(Range::All).unwrap().len(), 1);
        assert_eq!(s.recent(2).unwrap().len(), 2);

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.csv");
        assert_eq!(s.export_csv(Range::All, &p).unwrap(), 3);

        s.reset().unwrap();
        assert_eq!(s.summary(Range::All).unwrap().requests, 0);
    }
}
