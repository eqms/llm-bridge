// LLM Bridge – Oberfläche (Vanilla JS, kein Build-Schritt nötig)
"use strict";

const { t } = window.i18n;
const TAURI = window.__TAURI__;

// ---------------------------------------------------------------------------
// Backend-Anbindung (im Browser ohne Tauri: Demo-Daten für die Vorschau)
// ---------------------------------------------------------------------------
const invoke = TAURI ? TAURI.core.invoke : (cmd, args) => mockInvoke(cmd, args);
const listen = TAURI ? TAURI.event.listen : () => Promise.resolve(() => {});

// ---------------------------------------------------------------------------
// Zustand
// ---------------------------------------------------------------------------
const S = {
  view: "overview",
  status: null,
  config: null,
  saved: "",
  keys: {},
  found: {},          // upstreamId -> [modell-ids]
  tests: {},          // alias -> TestResult | "pending"
  range: "7d",
  report: null,
  overview: null,     // Report für 24 h
  autostart: false,
};

const PRESETS = {
  "langdock-eu": { id: "langdock-eu", name: "LangDock EU", base_url: "https://api.langdock.com/anthropic/eu", auth: "bearer" },
  "langdock-us": { id: "langdock-us", name: "LangDock US", base_url: "https://api.langdock.com/anthropic/us", auth: "bearer" },
  anthropic: { id: "anthropic", name: "Anthropic", base_url: "https://api.anthropic.com", auth: "x-api-key" },
  custom: { id: "custom", name: "", base_url: "https://", auth: "bearer" },
};

const ICONS = {
  overview: '<path d="M3 12h4l3-8 4 16 3-8h4"/>',
  stats: '<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>',
  upstreams: '<path d="M4 15a8 8 0 0 1 16 0M2 15h20M8 15v-3M16 15v-3"/>',
  models: '<path d="M4 6h16M4 12h10M4 18h6M18 14l3 3-3 3"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  play: '<path d="M7 4l13 8-13 8z"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  download: '<path d="M12 3v12M7 10l5 5 5-5M4 21h16"/>',
};
const icon = (name) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;

const esc = (v) =>
  String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const N = (n) => window.i18n.num(n);
const $ = (sel) => document.querySelector(sel);
const dirty = () => S.config && JSON.stringify(S.config) !== S.saved;
const clone = (o) => JSON.parse(JSON.stringify(o));

// ---------------------------------------------------------------------------
// Rückmeldungen
// ---------------------------------------------------------------------------
function toast(msg, kind = "") {
  const el = document.createElement("div");
  el.className = `toast ${kind}`;
  el.textContent = msg;
  $("#toasts").appendChild(el);
  setTimeout(() => el.remove(), kind === "err" ? 7000 : 3200);
}

async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
  toast(t("toast.copied", { value: text.length > 48 ? text.slice(0, 48) + "…" : text }), "ok");
}

const errText = (e) => (typeof e === "string" ? e : e?.message || JSON.stringify(e));

// ---------------------------------------------------------------------------
// Laden
// ---------------------------------------------------------------------------
async function loadAll() {
  const [status, config] = await Promise.all([invoke("get_status"), invoke("get_config")]);
  S.status = status;
  S.config = config;
  S.saved = JSON.stringify(config);
  window.i18n.set(config.ui.language);
  await Promise.all([loadKeys(), loadReports(), invoke("get_autostart").then((v) => (S.autostart = v)).catch(() => {})]);
}

async function loadKeys() {
  try { S.keys = await invoke("key_status"); } catch (e) { S.keys = {}; }
}

async function loadReports() {
  const [overview, report] = await Promise.all([invoke("get_report", { range: "24h" }), invoke("get_report", { range: S.range })]);
  S.overview = overview;
  S.report = report;
}

async function saveConfig(silent = false) {
  try {
    S.status = await invoke("save_config", { config: S.config });
    S.saved = JSON.stringify(S.config);
    window.i18n.set(S.config.ui.language);
    await loadKeys();
    if (!silent) toast(t("toast.saved"), "ok");
    render();
    return true;
  } catch (e) {
    toast(t("toast.save_failed", { error: errText(e) }), "err");
    return false;
  }
}

async function ensureSaved() {
  return dirty() ? saveConfig(true) : true;
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------
const VIEWS = ["overview", "stats", "upstreams", "models", "settings"];

function renderChrome() {
  $("#nav").innerHTML = VIEWS.map(
    (v) => `<button type="button" data-action="nav" data-view="${v}" ${S.view === v ? 'aria-current="page"' : ""}>${icon(v === "stats" ? "stats" : v)}<span>${esc(t(`nav.${v}`))}</span></button>`
  ).join("");
  $("#brand-sub").textContent = t("app.tagline");
  const st = S.status;
  const running = st?.running;
  $("#status-pill").innerHTML = `<span class="dot ${running ? "on" : st?.last_error ? "err" : ""}"></span><span>${esc(running ? t("status.running_short") : t("status.stopped_short"))}</span>`;
  $("#status-pill").title = running ? st.address : t("status.click_to_start");
  $("#version").textContent = st ? `v${st.version}` : "";
  const bar = $("#savebar");
  bar.hidden = !dirty();
  $("#savebar-text").textContent = t("save.unsaved");
  $("#discard").textContent = t("save.discard");
  $("#save").textContent = t("save.save");
}

function render() {
  renderChrome();
  const view = { overview: viewOverview, stats: viewStats, upstreams: viewUpstreams, models: viewModels, settings: viewSettings }[S.view];
  const main = $("#view");
  const scroll = main.scrollTop;
  main.innerHTML = view();
  main.scrollTop = scroll;
  drawCharts();
}

function head(title, lead, actions = "") {
  return `<div class="page-head"><div><h1>${esc(title)}</h1>${lead ? `<p class="lead">${lead}</p>` : ""}</div><div class="row">${actions}</div></div>`;
}

function kpiTiles(sum, opts = {}) {
  const tiles = [
    { label: t("kpi.requests"), value: N(sum.requests), sub: sum.errors ? t("kpi.errors", { n: N(sum.errors) }) : t("kpi.no_errors") },
    { label: t("kpi.input"), value: N(sum.input_tokens + sum.cache_read_tokens + sum.cache_write_tokens), sub: sum.cache_read_tokens || sum.cache_write_tokens ? t("kpi.cache_detail", { read: N(sum.cache_read_tokens), write: N(sum.cache_write_tokens) }) : t("kpi.input_sub"), swatch: "var(--input-color)" },
    { label: t("kpi.output"), value: N(sum.output_tokens), sub: t("kpi.output_sub"), swatch: "var(--output-color)" },
    { label: t("kpi.total"), value: N(sum.total_tokens), sub: sum.requests ? t("kpi.per_request", { n: N(Math.round(sum.total_tokens / sum.requests)) }) : "–" },
  ];
  if (opts.duration) tiles.push({ label: t("kpi.avg_duration"), value: `${N(sum.avg_duration_ms)} ms`, sub: t("kpi.duration_sub") });
  return `<div class="kpis">${tiles
    .map((k) => `<div class="card kpi"><div class="kpi-label">${k.swatch ? `<i class="swatch" style="background:${k.swatch}"></i>` : ""}${esc(k.label)}</div><div class="kpi-value">${esc(k.value)}</div><div class="kpi-sub">${esc(k.sub)}</div></div>`)
    .join("")}</div>`;
}

function statusBadge(r) {
  if (r.status === 499) return `<span class="badge warn" title="${esc(r.error)}">${esc(t("req.aborted"))}</span>`;
  if (r.status >= 400 || r.error) return `<span class="badge err" title="${esc(r.error)}">${r.status}</span>`;
  return `<span class="badge ok">${r.status}</span>`;
}

function recentTable(rows, limit) {
  rows = rows.slice(0, limit);
  if (!rows.length) return `<div class="empty">${esc(t("stats.empty"))}</div>`;
  return `<div class="table-wrap"><table><thead><tr>
      <th>${esc(t("req.time"))}</th><th>${esc(t("req.client"))}</th><th>${esc(t("req.model"))}</th>
      <th class="r">${esc(t("req.in"))}</th><th class="r">${esc(t("req.out"))}</th><th class="r">${esc(t("req.duration"))}</th><th>${esc(t("req.status"))}</th>
    </tr></thead><tbody>${rows
      .map(
        (r) => `<tr>
        <td class="num small">${esc(window.i18n.dateTime(r.ts))}</td>
        <td>${esc(r.client)}</td>
        <td><span class="mono">${esc(r.alias)}</span>${r.alias !== r.model ? ` <span class="muted small">→ ${esc(r.model)}</span>` : ""}${r.stream ? ` <span class="badge neutral" title="${esc(t("req.stream"))}">SSE</span>` : ""}</td>
        <td class="r num">${N(r.input_tokens + r.cache_read_tokens + r.cache_write_tokens)}</td>
        <td class="r num">${N(r.output_tokens)}</td>
        <td class="r num">${N(r.duration_ms)} ms</td>
        <td>${statusBadge(r)}</td></tr>`
      )
      .join("")}</tbody></table></div>`;
}

// --- Übersicht --------------------------------------------------------------
function viewOverview() {
  const st = S.status;
  const cfg = S.config;
  const token = cfg.server.access_token;
  const aliases = cfg.models.map((m) => m.alias);
  const missingKeys = cfg.upstreams.filter((u) => (S.keys[u.id] || "none") === "none" && cfg.models.some((m) => m.upstream === u.id));
  return `${head(t("overview.title"), esc(t("overview.lead")))}
  <div class="stack">
    <div class="card hero">
      <span class="dot big-dot ${st.running ? "on" : st.last_error ? "err" : ""}"></span>
      <div>
        <div class="hero-title">${esc(st.running ? t("status.running") : t("status.stopped"))}</div>
        <div class="muted small mono">${esc(st.running ? `http://${st.address}/v1` : t("status.not_listening"))}</div>
      </div>
      <div class="spacer"></div>
      ${st.running
        ? `<button class="btn" data-action="stop">${icon("stop")}${esc(t("action.stop"))}</button>`
        : `<button class="btn primary" data-action="start">${icon("play")}${esc(t("action.start"))}</button>`}
    </div>
    ${st.last_error ? `<div class="banner err">${esc(st.last_error)}</div>` : ""}
    ${missingKeys.length ? `<div class="banner warn">${esc(t("overview.missing_keys", { list: missingKeys.map((u) => u.name || u.id).join(", ") }))} <a href="#" data-action="nav" data-view="upstreams">${esc(t("overview.goto_upstreams"))}</a></div>` : ""}
    <div class="card">
      <h2>${esc(t("overview.client_setup"))}</h2>
      <p class="muted small" style="margin:-6px 0 14px">${esc(t("overview.client_hint"))}</p>
      <div class="copy-list">
        <span class="label">${esc(t("overview.base_url"))}</span>
        <span class="copy-value">${esc(st.base_url)}</span>
        <button class="btn sm" data-action="copy" data-value="${esc(st.base_url)}">${icon("copy")}${esc(t("action.copy"))}</button>
        <span class="label">${esc(t("overview.api_key"))}</span>
        <span class="copy-value">${esc(token || t("overview.any_key"))}</span>
        <button class="btn sm" data-action="copy" data-value="${esc(token || "sk-local")}">${icon("copy")}${esc(t("action.copy"))}</button>
        <span class="label">${esc(t("overview.model_name"))}</span>
        <div class="chips">${aliases.length ? aliases.map((a) => `<button class="chip" data-action="copy" data-value="${esc(a)}">${esc(a)}</button>`).join("") : `<span class="muted small">${esc(t("overview.no_models"))}</span>`}</div>
        <span></span>
      </div>
    </div>
    <div>
      <div class="row" style="margin-bottom:10px"><h2 style="margin:0">${esc(t("overview.last_24h"))}</h2><div class="spacer"></div><button class="btn sm ghost" data-action="nav" data-view="stats">${esc(t("overview.all_stats"))} →</button></div>
      ${kpiTiles(S.overview.summary)}
    </div>
    <div class="card"><h2>${esc(t("overview.recent"))}</h2>${recentTable(S.overview.recent, 6)}</div>
  </div>`;
}

// --- Statistik --------------------------------------------------------------
const RANGES = ["24h", "7d", "30d", "90d", "all"];

function groupTable(rows, keyLabel) {
  if (!rows.length) return `<div class="empty">${esc(t("stats.empty"))}</div>`;
  const total = rows.reduce((a, r) => a + r.input_tokens + r.output_tokens, 0) || 1;
  return `<div class="table-wrap"><table><thead><tr><th>${esc(keyLabel)}</th><th class="r">${esc(t("kpi.requests"))}</th><th class="r">${esc(t("req.in"))}</th><th class="r">${esc(t("req.out"))}</th><th>${esc(t("stats.share"))}</th></tr></thead><tbody>${rows
    .map((r) => {
      const share = ((r.input_tokens + r.output_tokens) / total) * 100;
      return `<tr><td class="mono">${esc(r.key || "–")}</td><td class="r num">${N(r.requests)}</td><td class="r num">${N(r.input_tokens)}</td><td class="r num">${N(r.output_tokens)}</td><td title="${share.toFixed(1)} %"><div class="share"><i style="width:${share.toFixed(1)}%"></i></div></td></tr>`;
    })
    .join("")}</tbody></table></div>`;
}

function viewStats() {
  const r = S.report;
  const seg = `<div class="segmented" role="group">${RANGES.map((x) => `<button type="button" data-action="range" data-range="${x}" aria-pressed="${S.range === x}">${esc(t(`range.${x}`))}</button>`).join("")}</div>`;
  const actions = `${seg}<button class="btn" data-action="export">${icon("download")}${esc(t("stats.export"))}</button><button class="btn danger" data-action="reset-stats" data-confirm="${esc(t("stats.reset_confirm"))}">${esc(t("stats.reset"))}</button>`;
  return `${head(t("stats.title"), esc(t("stats.lead")), actions)}
  <div class="stack">
    ${kpiTiles(r.summary, { duration: true })}
    <div class="card">
      <div class="row" style="margin-bottom:8px"><h2 style="margin:0">${esc(t(r.range === "24h" ? "stats.per_hour" : "stats.per_day"))}</h2><div class="spacer"></div>
        <div class="legend"><span><i class="swatch" style="background:var(--input-color)"></i>${esc(t("kpi.input"))}</span><span><i class="swatch" style="background:var(--output-color)"></i>${esc(t("kpi.output"))}</span></div></div>
      <div id="chart"></div>
    </div>
    <div class="grid-2">
      <div class="card"><h2>${esc(t("stats.by_model"))}</h2>${groupTable(r.by_model, t("req.model"))}</div>
      <div class="card"><h2>${esc(t("stats.by_client"))}</h2>${groupTable(r.by_client, t("req.client"))}</div>
    </div>
    <div class="card"><h2>${esc(t("stats.recent"))}</h2>${recentTable(r.recent, 100)}</div>
  </div>`;
}

function niceMax(v) {
  if (v <= 0) return 1;
  const p = Math.pow(10, Math.floor(Math.log10(v)));
  for (const m of [1, 2, 2.5, 5, 10]) if (m * p >= v) return m * p;
  return 10 * p;
}

function bucketLabel(label, hourly) {
  if (hourly) return label.slice(11, 16);
  const d = new Date(`${label}T00:00:00`);
  return new Intl.DateTimeFormat(window.i18n.lang, { day: "2-digit", month: "2-digit" }).format(d);
}

function drawCharts() {
  const host = $("#chart");
  if (!host || !S.report) return;
  const series = S.report.series;
  const hourly = S.report.range === "24h";
  const W = Math.max(320, host.clientWidth);
  const H = 230;
  const pad = { l: 52, r: 6, t: 10, b: 26 };
  const iw = W - pad.l - pad.r;
  const ih = H - pad.t - pad.b;
  const max = niceMax(Math.max(0, ...series.map((b) => b.input_tokens + b.output_tokens)));
  const n = Math.max(1, series.length);
  const bw = iw / n;
  const barW = Math.max(1, Math.min(28, bw * 0.68));
  const y = (v) => pad.t + ih - (v / max) * ih;
  const every = Math.ceil(n / Math.max(4, Math.floor(iw / 64)));
  let svg = `<svg class="chart" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(t("stats.chart_label"))}"><g class="grid">`;
  for (const f of [0, 0.25, 0.5, 0.75, 1]) {
    const v = max * f;
    svg += `<line x1="${pad.l}" x2="${W - pad.r}" y1="${y(v)}" y2="${y(v)}"/><text x="${pad.l - 8}" y="${y(v) + 4}" text-anchor="end">${esc(window.i18n.compact(v))}</text>`;
  }
  svg += "</g>";
  series.forEach((b, i) => {
    const x = pad.l + i * bw + (bw - barW) / 2;
    const hin = (b.input_tokens / max) * ih;
    const hout = (b.output_tokens / max) * ih;
    const base = pad.t + ih;
    const tip = `${b.label}\n${t("kpi.requests")}: ${N(b.requests)}\n${t("kpi.input")}: ${N(b.input_tokens)}\n${t("kpi.output")}: ${N(b.output_tokens)}`;
    svg += `<g class="bar"><title>${esc(tip)}</title><rect x="${pad.l + i * bw}" y="${pad.t}" width="${bw}" height="${ih}" fill="transparent"/>`;
    if (hin > 0) svg += `<rect class="in" x="${x}" y="${base - hin}" width="${barW}" height="${hin}" rx="1.5"/>`;
    if (hout > 0) svg += `<rect class="out" x="${x}" y="${base - hin - hout}" width="${barW}" height="${hout}" rx="1.5"/>`;
    svg += "</g>";
    if (i % every === 0 || i === n - 1) {
      svg += `<text x="${pad.l + i * bw + bw / 2}" y="${H - 8}" text-anchor="middle">${esc(bucketLabel(b.label, hourly))}</text>`;
    }
  });
  host.innerHTML = svg + "</svg>";
}

// --- Upstreams --------------------------------------------------------------
function keyBadge(id) {
  const k = S.keys[id] || "none";
  if (k === "keyring") return `<span class="badge ok">${esc(t("key.keyring"))}</span>`;
  if (k === "env") return `<span class="badge ok">${esc(t("key.env"))}</span>`;
  return `<span class="badge warn">${esc(t("key.missing"))}</span>`;
}

function viewUpstreams() {
  const cfg = S.config;
  const add = Object.keys(PRESETS)
    .map((p) => `<button class="btn sm" data-action="add-upstream" data-preset="${p}">${icon("plus")}${esc(t(`preset.${p}`))}</button>`)
    .join("");
  const cards = cfg.upstreams
    .map((u, i) => {
      const found = S.found[u.id];
      return `<div class="card up-card">
      <div class="up-head"><span class="up-title">${esc(u.name || u.id || t("upstream.unnamed"))}</span>${keyBadge(u.id)}<div class="spacer"></div>
        <button class="btn sm" data-action="fetch-models" data-index="${i}">${esc(t("upstream.fetch_models"))}</button>
        <button class="btn sm danger" data-action="remove-upstream" data-index="${i}" data-confirm="${esc(t("upstream.remove_confirm"))}">${esc(t("action.remove"))}</button></div>
      <div class="form-grid">
        <label class="field"><span>${esc(t("upstream.name"))}</span><input type="text" data-bind="upstreams.${i}.name" value="${esc(u.name)}"></label>
        <label class="field"><span>${esc(t("upstream.id"))}</span><input type="text" data-bind="upstreams.${i}.id" value="${esc(u.id)}" spellcheck="false"></label>
        <label class="field" style="grid-column: span 2"><span>${esc(t("upstream.base_url"))}</span><input type="url" data-bind="upstreams.${i}.base_url" value="${esc(u.base_url)}" spellcheck="false"><small>${esc(t("upstream.base_url_hint"))}</small></label>
        <label class="field"><span>${esc(t("upstream.auth"))}</span><select data-bind="upstreams.${i}.auth">
          <option value="bearer" ${u.auth === "bearer" ? "selected" : ""}>Authorization: Bearer</option>
          <option value="x-api-key" ${u.auth === "x-api-key" ? "selected" : ""}>x-api-key</option></select></label>
        <label class="field"><span>anthropic-version</span><input type="text" data-bind="upstreams.${i}.anthropic_version" value="${esc(u.anthropic_version)}" spellcheck="false"></label>
        <label class="field"><span>${esc(t("upstream.env"))}</span><input type="text" data-bind="upstreams.${i}.api_key_env" value="${esc(u.api_key_env)}" placeholder="${esc(envName(u.id))}" spellcheck="false"></label>
      </div>
      <div class="key-row">
        <label class="field"><span>${esc(t("upstream.api_key"))}</span><input type="password" id="key-${i}" placeholder="${esc(t((S.keys[u.id] || "none") === "none" ? "upstream.key_placeholder" : "upstream.key_placeholder_set"))}" autocomplete="off"></label>
        <button class="btn primary" data-action="save-key" data-index="${i}">${esc(t("upstream.save_key"))}</button>
        <button class="btn" data-action="delete-key" data-index="${i}" ${S.keys[u.id] === "keyring" ? "" : "disabled"}>${esc(t("upstream.delete_key"))}</button>
      </div>
      ${found ? `<div class="models-found"><div class="small muted" style="margin-bottom:8px">${esc(t("upstream.found", { n: found.length }))}</div><div class="chips">${found.map((m) => `<button class="chip" data-action="use-model" data-index="${i}" data-model="${esc(m)}" title="${esc(t("upstream.use_model"))}">${esc(m)}</button>`).join("")}</div></div>` : ""}
    </div>`;
    })
    .join("");
  return `${head(t("upstreams.title"), esc(t("upstreams.lead")))}
  <div class="stack">${cards || `<div class="card empty">${esc(t("upstreams.empty"))}</div>`}
    <div class="row"><span class="muted small">${esc(t("upstreams.add"))}</span>${add}</div></div>`;
}

function envName(id) {
  return "LLM_BRIDGE_KEY_" + String(id || "").toUpperCase().replace(/[^A-Z0-9]/g, "_");
}

// --- Modelle ----------------------------------------------------------------
function viewModels() {
  const cfg = S.config;
  const ups = cfg.upstreams;
  const rows = cfg.models
    .map((m, i) => {
      const tr = S.tests[m.alias];
      const result =
        tr === "pending"
          ? `<span class="test-result muted">${esc(t("models.testing"))}</span>`
          : tr
          ? `<span class="test-result ${tr.ok ? "ok" : "err"}" title="${esc(tr.message)}">${tr.ok ? `✓ ${N(tr.latency_ms)} ms` : `✕ ${esc(tr.message.slice(0, 90))}`}</span>`
          : "";
      const list = (S.found[m.upstream] || []).map((x) => `<option value="${esc(x)}"></option>`).join("");
      return `<tr>
        <td><input type="text" data-bind="models.${i}.alias" value="${esc(m.alias)}" spellcheck="false" style="min-width:90px"></td>
        <td><select data-bind="models.${i}.upstream" style="min-width:130px">${ups.map((u) => `<option value="${esc(u.id)}" ${u.id === m.upstream ? "selected" : ""}>${esc(u.name || u.id)}</option>`).join("")}</select></td>
        <td><input type="text" data-bind="models.${i}.model" value="${esc(m.model)}" list="dl-${i}" spellcheck="false" style="min-width:190px"><datalist id="dl-${i}">${list}</datalist></td>
        <td><input type="number" min="1" data-bind="models.${i}.max_tokens" data-type="optnum" value="${m.max_tokens ?? ""}" placeholder="${N(cfg.defaults.max_tokens)}" style="width:86px"></td>
        <td><div class="row" style="flex-wrap:nowrap"><button class="btn sm" data-action="test-model" data-alias="${esc(m.alias)}">${esc(t("models.test"))}</button>
          <button class="btn sm danger" data-action="remove-model" data-index="${i}" title="${esc(t("action.remove"))}" aria-label="${esc(t("action.remove"))}">✕</button></div>${result}</td>
      </tr>`;
    })
    .join("");
  return `${head(t("models.title"), esc(t("models.lead")), `<button class="btn" data-action="add-model" ${ups.length ? "" : "disabled"}>${icon("plus")}${esc(t("models.add"))}</button>`)}
  <div class="stack">
    <div class="card">${cfg.models.length ? `<div class="table-wrap"><table>
      <thead><tr><th>${esc(t("models.alias"))}</th><th>${esc(t("models.upstream"))}</th><th>${esc(t("models.model_id"))}</th><th>max_tokens</th><th></th></tr></thead>
      <tbody>${rows}</tbody></table></div>` : `<div class="empty">${esc(t("models.empty"))}</div>`}</div>
    <div class="banner info">${t("models.hint")}</div>
  </div>`;
}

// --- Einstellungen ----------------------------------------------------------
function viewSettings() {
  const cfg = S.config;
  const st = S.status;
  const langs = window.i18n.available();
  const check = (bind, label, hint, value) =>
    `<label class="check"><input type="checkbox" data-bind="${bind}" ${value ? "checked" : ""}><span>${esc(label)}<small>${esc(hint)}</small></span></label>`;
  return `${head(t("settings.title"), "")}
  <div class="stack">
    <div class="card"><h2>${esc(t("settings.general"))}</h2>
      <div class="form-grid">
        <label class="field" style="max-width:320px"><span>${esc(t("settings.language"))}</span><select data-bind="ui.language">
          <option value="auto" ${cfg.ui.language === "auto" ? "selected" : ""}>${esc(t("settings.language_auto"))}</option>
          ${langs.map((l) => `<option value="${l.code}" ${cfg.ui.language === l.code ? "selected" : ""}>${esc(l.name)}</option>`).join("")}
        </select></label>
      </div>
      <div style="margin-top:8px">
        ${check("ui.autostart_proxy", t("settings.autostart_proxy"), t("settings.autostart_proxy_hint"), cfg.ui.autostart_proxy)}
        ${check("ui.start_minimized", t("settings.start_minimized"), t("settings.start_minimized_hint"), cfg.ui.start_minimized)}
        ${check("ui.auto_update", t("settings.auto_update"), t("settings.auto_update_hint"), cfg.ui.auto_update)}
        <label class="check"><input type="checkbox" data-action="autostart" ${S.autostart ? "checked" : ""}><span>${esc(t("settings.login_item"))}<small>${esc(t("settings.login_item_hint"))}</small></span></label>
      </div>
    </div>
    <div class="card"><h2>${esc(t("settings.server"))}</h2>
      <div class="form-grid">
        <label class="field"><span>${esc(t("settings.host"))}</span><input type="text" data-bind="server.host" value="${esc(cfg.server.host)}" spellcheck="false"><small>${esc(t("settings.host_hint"))}</small></label>
        <label class="field"><span>${esc(t("settings.port"))}</span><input type="number" min="1" max="65535" data-bind="server.port" data-type="num" value="${cfg.server.port}"></label>
        <label class="field" style="grid-column: span 2"><span>${esc(t("settings.token"))}</span>
          <div class="row" style="flex-wrap:nowrap"><input type="text" data-bind="server.access_token" value="${esc(cfg.server.access_token)}" placeholder="${esc(t("settings.token_placeholder"))}" spellcheck="false" class="mono">
          <button class="btn" data-action="gen-token">${esc(t("settings.token_generate"))}</button></div>
          <small>${esc(t("settings.token_hint"))}</small></label>
      </div>
      ${cfg.server.host !== "127.0.0.1" && cfg.server.host !== "localhost" ? `<div class="banner warn" style="margin-top:12px">${esc(t("settings.host_warning"))}</div>` : ""}
    </div>
    <div class="card"><h2>${esc(t("settings.requests"))}</h2>
      <div class="form-grid">
        <label class="field"><span>${esc(t("settings.max_tokens"))}</span><input type="number" min="1" data-bind="defaults.max_tokens" data-type="num" value="${cfg.defaults.max_tokens}"><small>${esc(t("settings.max_tokens_hint"))}</small></label>
        <label class="field"><span>${esc(t("settings.timeout"))}</span><input type="number" min="10" data-bind="defaults.request_timeout_secs" data-type="num" value="${cfg.defaults.request_timeout_secs}"><small>${esc(t("settings.timeout_hint"))}</small></label>
      </div>
    </div>
    <div class="card"><h2>${esc(t("settings.about"))}</h2>
      <div class="copy-list">
        <span class="label">${esc(t("settings.version"))}</span><span class="copy-value">${esc(st.version)}</span><span></span>
        <span class="label">${esc(t("settings.config_file"))}</span><span class="copy-value">${esc(st.config_path)}</span><button class="btn sm" data-action="copy" data-value="${esc(st.config_path)}">${icon("copy")}</button>
        <span class="label">${esc(t("settings.db_file"))}</span><span class="copy-value">${esc(st.db_path)}</span><button class="btn sm" data-action="copy" data-value="${esc(st.db_path)}">${icon("copy")}</button>
      </div>
      <div class="row" style="margin-top:14px"><span class="muted small">${esc(t("settings.privacy"))}</span><div class="spacer"></div><button class="btn danger" data-action="quit">${esc(t("settings.quit"))}</button></div>
    </div>
  </div>`;
}

// ---------------------------------------------------------------------------
// Eingaben
// ---------------------------------------------------------------------------
function setPath(obj, path, value) {
  const parts = path.split(".");
  let o = obj;
  for (let i = 0; i < parts.length - 1; i++) o = o[parts[i]];
  o[parts[parts.length - 1]] = value;
}

function onInput(e) {
  const el = e.target;
  const bind = el.dataset.bind;
  if (!bind) return;
  let value;
  if (el.type === "checkbox") value = el.checked;
  else if (el.dataset.type === "num") value = el.value === "" ? 0 : Number(el.value);
  else if (el.dataset.type === "optnum") value = el.value === "" ? null : Number(el.value);
  else value = el.value;

  // Upstream-ID umbenannt → Zuordnungen mitziehen
  const m = bind.match(/^upstreams\.(\d+)\.id$/);
  if (m) {
    const old = S.config.upstreams[+m[1]].id;
    S.config.models.forEach((r) => { if (r.upstream === old) r.upstream = value; });
    if (S.found[old]) { S.found[value] = S.found[old]; delete S.found[old]; }
  }
  setPath(S.config, bind, value);
  if (bind === "ui.language") {
    window.i18n.set(value);
    render();
  } else if (el.tagName === "SELECT" || el.type === "checkbox") {
    render();
  } else {
    renderChrome();
  }
}

const pendingConfirm = new WeakMap();

async function onClick(e) {
  const el = e.target.closest("[data-action]");
  if (!el) return;
  const a = el.dataset.action;
  if (el.tagName === "A") e.preventDefault();

  // Zweistufige Bestätigung statt Dialog
  if (el.dataset.confirm && !pendingConfirm.get(el)) {
    const original = el.innerHTML;
    el.textContent = el.dataset.confirm;
    pendingConfirm.set(el, true);
    setTimeout(() => { if (el.isConnected) { el.innerHTML = original; pendingConfirm.delete(el); } }, 4000);
    return;
  }

  const i = el.dataset.index !== undefined ? +el.dataset.index : -1;
  try {
    switch (a) {
      case "nav":
        S.view = el.dataset.view;
        if (S.view === "stats" || S.view === "overview") await loadReports();
        render();
        $("#view").scrollTop = 0;
        break;
      case "copy":
        await copy(el.dataset.value);
        break;
      case "start":
        el.disabled = true;
        try { S.status = await invoke("start_proxy"); } catch (err) { toast(t("toast.start_failed", { error: errText(err) }), "err"); S.status = await invoke("get_status"); }
        render();
        break;
      case "stop":
        S.status = await invoke("stop_proxy");
        render();
        break;
      case "range":
        S.range = el.dataset.range;
        S.report = await invoke("get_report", { range: S.range });
        render();
        break;
      case "export": {
        const path = await invoke("export_csv", { range: S.range });
        toast(t("toast.exported", { path }), "ok");
        break;
      }
      case "reset-stats":
        await invoke("reset_stats");
        await loadReports();
        render();
        toast(t("toast.stats_reset"), "ok");
        break;
      case "add-upstream": {
        const p = clone(PRESETS[el.dataset.preset]);
        let id = p.id, n = 2;
        while (S.config.upstreams.some((u) => u.id === id)) id = `${p.id}-${n++}`;
        S.config.upstreams.push({ anthropic_version: "2023-06-01", api_key_env: "", ...p, id });
        render();
        break;
      }
      case "remove-upstream": {
        const id = S.config.upstreams[i].id;
        S.config.upstreams.splice(i, 1);
        S.config.models = S.config.models.filter((m) => m.upstream !== id);
        render();
        break;
      }
      case "save-key": {
        const input = $(`#key-${i}`);
        const key = input.value.trim();
        if (!key) { toast(t("toast.key_empty"), "err"); input.focus(); break; }
        await invoke("set_api_key", { upstreamId: S.config.upstreams[i].id, key });
        input.value = "";
        await loadKeys();
        render();
        toast(t("toast.key_saved"), "ok");
        break;
      }
      case "delete-key":
        await invoke("delete_api_key", { upstreamId: S.config.upstreams[i].id });
        await loadKeys();
        render();
        toast(t("toast.key_deleted"), "ok");
        break;
      case "fetch-models": {
        if (!(await ensureSaved())) break;
        const up = S.config.upstreams[i];
        el.disabled = true;
        el.textContent = t("upstream.fetching");
        try {
          S.found[up.id] = await invoke("upstream_models", { upstreamId: up.id });
          if (!S.found[up.id].length) toast(t("toast.no_models"));
        } catch (err) {
          toast(t("toast.fetch_failed", { error: errText(err) }), "err");
        }
        render();
        break;
      }
      case "use-model": {
        const up = S.config.upstreams[i];
        const model = el.dataset.model;
        const existing = S.config.models.find((m) => m.upstream === up.id && m.model === model);
        if (existing) { toast(t("toast.model_exists", { alias: existing.alias })); break; }
        let base = model.replace(/^claude-/, "").replace(/-default$/, "").replace(/-\d{8}$/, "") || model;
        let alias = base, n = 2;
        while (S.config.models.some((m) => m.alias.toLowerCase() === alias.toLowerCase())) alias = `${base}-${n++}`;
        S.config.models.push({ alias, upstream: up.id, model });
        toast(t("toast.model_added", { alias, model }), "ok");
        render();
        break;
      }
      case "add-model":
        S.config.models.push({ alias: "", upstream: S.config.upstreams[0].id, model: "" });
        render();
        break;
      case "remove-model":
        S.config.models.splice(i, 1);
        render();
        break;
      case "test-model": {
        const alias = el.dataset.alias;
        if (!alias) break;
        if (!(await ensureSaved())) break;
        S.tests[alias] = "pending";
        render();
        S.tests[alias] = await invoke("test_model", { model: alias });
        render();
        break;
      }
      case "gen-token": {
        const bytes = crypto.getRandomValues(new Uint8Array(18));
        S.config.server.access_token = "lb-" + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
        render();
        break;
      }
      case "autostart":
        S.autostart = await invoke("set_autostart", { enabled: el.checked });
        el.checked = S.autostart;
        break;
      case "quit":
        await invoke("quit_app");
        break;
    }
  } catch (err) {
    toast(errText(err), "err");
    render();
  }
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------
let refreshTimer = null;
function scheduleRefresh() {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(async () => {
    if (S.view !== "stats" && S.view !== "overview") return;
    // Während der Eingabe nicht neu zeichnen
    if (document.activeElement && document.activeElement.matches("input, select")) return;
    await loadReports();
    render();
  }, 500);
}

async function init() {
  document.addEventListener("click", onClick);
  document.addEventListener("input", (e) => { if (e.target.tagName !== "SELECT" && e.target.type !== "checkbox") onInput(e); });
  document.addEventListener("change", (e) => { if (e.target.tagName === "SELECT" || e.target.type === "checkbox") onInput(e); });
  $("#save").addEventListener("click", () => saveConfig());
  $("#discard").addEventListener("click", () => {
    S.config = JSON.parse(S.saved);
    window.i18n.set(S.config.ui.language);
    render();
  });
  $("#status-pill").addEventListener("click", async () => {
    try {
      S.status = await invoke(S.status?.running ? "stop_proxy" : "start_proxy");
    } catch (err) {
      toast(t("toast.start_failed", { error: errText(err) }), "err");
      S.status = await invoke("get_status");
    }
    render();
  });
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (dirty()) saveConfig();
    }
  });
  let rt;
  window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(drawCharts, 120); });

  await listen("request-recorded", scheduleRefresh);
  await listen("stats-reset", scheduleRefresh);
  await listen("status-changed", (ev) => { S.status = ev.payload; if (!dirty()) S.config = JSON.parse(S.saved); renderChrome(); if (S.view === "overview") render(); });

  try {
    await loadAll();
  } catch (err) {
    $("#view").innerHTML = `<div class="banner err">${esc(errText(err))}</div>`;
    return;
  }
  render();
}

init();

// ---------------------------------------------------------------------------
// Demo-Backend für die Vorschau im Browser (ohne Tauri)
// ---------------------------------------------------------------------------
function mockInvoke(cmd, args = {}) {
  const M = (window.__mock ||= (() => {
    const now = Date.now();
    const clients = ["MacWhisper", "MacWhisper", "MacWhisper", "curl", "OpenAI"];
    const recent = Array.from({ length: 40 }, (_, k) => {
      const i = 300 + Math.round(Math.random() * 1800);
      return {
        id: 40 - k, ts: now - k * 47 * 60_000, client: clients[k % clients.length], alias: k % 4 ? "haiku" : "sonnet",
        upstream: "langdock-eu", model: k % 4 ? "claude-haiku-5-5-default" : "claude-sonnet-4-6-default",
        input_tokens: i, output_tokens: Math.round(i * (0.6 + Math.random() * 0.5)), cache_read_tokens: k % 5 === 0 ? 120 : 0,
        cache_write_tokens: 0, duration_ms: 400 + Math.round(Math.random() * 1600), status: k === 3 ? 404 : k === 9 ? 499 : 200,
        stream: k % 3 !== 0, error: k === 3 ? "Upstream-Fehler (404): Not found" : k === 9 ? "Verbindung vom Client abgebrochen" : null,
      };
    });
    return {
      running: true,
      config: {
        server: { host: "127.0.0.1", port: 4000, access_token: "" },
        defaults: { max_tokens: 4096, request_timeout_secs: 300 },
        ui: { language: "auto", start_minimized: false, autostart_proxy: true, auto_update: true },
        upstreams: [{ id: "langdock-eu", name: "LangDock EU", base_url: "https://api.langdock.com/anthropic/eu", auth: "bearer", anthropic_version: "2023-06-01", api_key_env: "" }],
        models: [
          { alias: "haiku", upstream: "langdock-eu", model: "claude-haiku-5-5-default" },
          { alias: "sonnet", upstream: "langdock-eu", model: "claude-sonnet-4-6-default" },
        ],
      },
      recent,
    };
  })());
  const report = (range) => {
    const hourly = range === "24h";
    const n = { "24h": 24, "7d": 7, "30d": 30, "90d": 90, all: 45 }[range] || 7;
    const series = Array.from({ length: n }, (_, k) => {
      const d = new Date(Date.now() - (n - 1 - k) * (hourly ? 3_600_000 : 86_400_000));
      const pad = (x) => String(x).padStart(2, "0");
      const label = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` + (hourly ? ` ${pad(d.getHours())}:00` : "");
      const req = Math.max(0, Math.round((hourly ? 3 : 25) * (0.4 + Math.sin(k / 2.3) ** 2 + Math.random() * 0.4)));
      return { label, requests: req, input_tokens: req * 900, output_tokens: req * 640 };
    });
    const tot = series.reduce((a, b) => ({ r: a.r + b.requests, i: a.i + b.input_tokens, o: a.o + b.output_tokens }), { r: 0, i: 0, o: 0 });
    return {
      range,
      summary: { requests: tot.r, errors: Math.round(tot.r * 0.02), input_tokens: tot.i - 2400, output_tokens: tot.o, cache_read_tokens: 2400, cache_write_tokens: 0, total_tokens: tot.i + tot.o, avg_duration_ms: 1140 },
      series,
      by_model: [{ key: "haiku", requests: Math.round(tot.r * 0.8), input_tokens: Math.round(tot.i * 0.7), output_tokens: Math.round(tot.o * 0.7) }, { key: "sonnet", requests: Math.round(tot.r * 0.2), input_tokens: Math.round(tot.i * 0.3), output_tokens: Math.round(tot.o * 0.3) }],
      by_client: [{ key: "MacWhisper", requests: Math.round(tot.r * 0.85), input_tokens: Math.round(tot.i * 0.85), output_tokens: Math.round(tot.o * 0.85) }, { key: "curl", requests: Math.round(tot.r * 0.15), input_tokens: Math.round(tot.i * 0.15), output_tokens: Math.round(tot.o * 0.15) }],
      recent: M.recent,
    };
  };
  const status = () => ({ running: M.running, address: M.running ? `127.0.0.1:${M.config.server.port}` : null, base_url: `http://127.0.0.1:${M.config.server.port}/v1`, last_error: null, version: "0.1.0", config_path: "~/Library/Application Support/ai.it-guy.LLM-Bridge/config.toml", db_path: "~/Library/Application Support/ai.it-guy.LLM-Bridge/stats.sqlite", language: "de" });
  const handlers = {
    get_status: status,
    get_config: () => clone(M.config),
    save_config: () => { M.config = clone(args.config); return status(); },
    start_proxy: () => { M.running = true; return status(); },
    stop_proxy: () => { M.running = false; return status(); },
    key_status: () => Object.fromEntries(M.config.upstreams.map((u, i) => [u.id, i === 0 ? "keyring" : "none"])),
    set_api_key: () => null,
    delete_api_key: () => null,
    test_model: () => ({ ok: true, status: 200, latency_ms: 612, message: "OK" }),
    upstream_models: () => ["claude-haiku-5-5-default", "claude-sonnet-4-6-default", "claude-opus-5-5-default"],
    get_report: () => report(args.range),
    reset_stats: () => null,
    export_csv: () => "~/Downloads/llm-bridge-stats.csv",
    get_autostart: () => false,
    set_autostart: () => args.enabled,
    quit_app: () => null,
  };
  return new Promise((res) => setTimeout(() => res(handlers[cmd]?.()), 30));
}
