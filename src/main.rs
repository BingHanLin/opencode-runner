//! Iced port of the opencode orchestrator GUI.
//!
//! Run with:  `cargo run --bin iced_preview`
//!
//! Backend modules (config / db / opencode / runner / scheduler) are shared
//! through the library crate and exercised the same way the egui binary
//! does — the same tokio runtime model, same on-disk state.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Timelike, Utc};
use iced::{
    Background, Border, Color, Element, Font, Length, Padding, Shadow, Subscription, Theme,
    alignment::{Horizontal, Vertical},
    border::Radius,
    keyboard,
    widget::{
        Space, button, checkbox, column, container, mouse_area, pick_list, row, scrollable, svg,
        text, text_editor, text_input,
    },
    window,
};
use tokio::runtime::Handle;
use tokio::sync::Notify;

use opencode_orchestrator::config::{self, Task, TasksFile};
use opencode_orchestrator::db::{Db, Run};
use opencode_orchestrator::opencode::storage::{self, Message as ChatMessage, Part as ChatPart};
use opencode_orchestrator::opencode::{Cli, Model};
use opencode_orchestrator::runner;
use opencode_orchestrator::scheduler::Scheduler;

// =============================================================================
//                                PALETTE
// =============================================================================

const BG: Color = rgb(0x0B, 0x0D, 0x12);
const SURFACE: Color = rgb(0x12, 0x15, 0x1D);
const SURFACE_2: Color = rgb(0x16, 0x1A, 0x24);
const BORDER_C: Color = rgb(0x23, 0x29, 0x36);
const TEXT_C: Color = rgb(0xE8, 0xEB, 0xF1);
const TEXT_MUTED: Color = rgb(0x9B, 0xA3, 0xB4);
const TEXT_FAINT: Color = rgb(0x60, 0x69, 0x7A);
const ACCENT: Color = rgb(0x8B, 0x7C, 0xFF);
const ACCENT_TEXT: Color = rgb(0xC4, 0xBC, 0xFF);

const SUCCESS: Color = rgb(0x4A, 0xD0, 0x95);
const INFO: Color = rgb(0x6F, 0xB1, 0xFA);
const WARN: Color = rgb(0xE5, 0xB1, 0x4F);
const ERROR: Color = rgb(0xEC, 0x71, 0x6D);

const RADIUS: f32 = 6.0;
const RADIUS_SM: f32 = 4.0;
const PAGE_PAD_X: f32 = 24.0;
const PAGE_PAD_Y: f32 = 18.0;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}
fn rgba(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

// =============================================================================
//                                ICONS
// =============================================================================

const ICON_PLUS: &[u8] = include_bytes!("../assets/icons/plus.svg");
const ICON_SAVE: &[u8] = include_bytes!("../assets/icons/save.svg");
const ICON_REVERT: &[u8] = include_bytes!("../assets/icons/rotate-ccw.svg");
const ICON_PLAY: &[u8] = include_bytes!("../assets/icons/play.svg");
const ICON_TRASH: &[u8] = include_bytes!("../assets/icons/trash.svg");
const ICON_FOLDER: &[u8] = include_bytes!("../assets/icons/folder.svg");
const ICON_REFRESH: &[u8] = include_bytes!("../assets/icons/refresh-cw.svg");
const ICON_WRENCH: &[u8] = include_bytes!("../assets/icons/wrench.svg");
const ICON_CLOCK: &[u8] = include_bytes!("../assets/icons/clock.svg");
const ICON_CALENDAR: &[u8] = include_bytes!("../assets/icons/calendar.svg");
const ICON_CIRCLE: &[u8] = include_bytes!("../assets/icons/circle.svg");
const ICON_ALERT: &[u8] = include_bytes!("../assets/icons/alert-triangle.svg");
const ICON_INFO: &[u8] = include_bytes!("../assets/icons/info.svg");
const ICON_CHECK: &[u8] = include_bytes!("../assets/icons/check.svg");
const ICON_X: &[u8] = include_bytes!("../assets/icons/x.svg");

fn icon(bytes: &'static [u8]) -> svg::Handle {
    svg::Handle::from_memory(bytes)
}
fn icon_svg<'a>(bytes: &'static [u8], size: f32, color: Color) -> Element<'a, Message> {
    svg(icon(bytes))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

// =============================================================================
//                                STATE
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubTab {
    Edit,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ScheduleKind {
    #[default]
    Manual,
    Cron,
    Once,
}

#[derive(Debug, Clone, Copy)]
enum StatusKind {
    Info,
    Success,
    Warn,
    Error,
}
impl StatusKind {
    fn color(self) -> Color {
        match self {
            Self::Info => INFO,
            Self::Success => SUCCESS,
            Self::Warn => WARN,
            Self::Error => ERROR,
        }
    }
    fn icon_bytes(self) -> &'static [u8] {
        match self {
            Self::Info => ICON_INFO,
            Self::Success => ICON_CHECK,
            Self::Warn => ICON_ALERT,
            Self::Error => ICON_X,
        }
    }
}

struct StatusMessage {
    kind: StatusKind,
    text: String,
    at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CronPreset {
    Hourly,    // every hour at :M
    #[default]
    Daily,     // every day at H:M
    Weekly,    // every <DOW> at H:M
    Monthly,   // every D of month at H:M
    Custom,    // raw cron expression
}

const DOW_OPTIONS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

struct EditState {
    loaded_id: Option<String>,
    dirty: bool,
    validation: Option<String>,
    pending_delete: bool,

    f_name: String,
    f_kind: ScheduleKind,
    f_working_dir: String,
    f_model: Option<String>,
    f_skip_perms: bool,
    f_enabled: bool,

    prompt: text_editor::Content,

    // Once picker (UTC):  YYYY-MM-DD  +  HH:MM
    once_date: String,
    once_time: String,

    // Cron picker
    cron_preset: CronPreset,
    cron_time: String,   // "HH:MM" — used by Hourly (minute only), Daily/Weekly/Monthly
    cron_dow: String,    // one of DOW_OPTIONS (Weekly)
    cron_day: String,    // 1..=31 (Monthly)
    cron_raw: String,    // raw 6-field expression (Custom)
}
impl Default for EditState {
    fn default() -> Self {
        Self {
            loaded_id: None,
            dirty: false,
            validation: None,
            pending_delete: false,
            f_name: String::new(),
            f_kind: ScheduleKind::Manual,
            f_working_dir: String::new(),
            f_model: None,
            f_skip_perms: false,
            f_enabled: true,
            prompt: text_editor::Content::new(),
            once_date: default_once_date(),
            once_time: "09:00".into(),
            cron_preset: CronPreset::Daily,
            cron_time: "09:00".into(),
            cron_dow: "MON".into(),
            cron_day: "1".into(),
            cron_raw: "0 0 9 * * *".into(),
        }
    }
}

fn default_once_date() -> String {
    // Default to "tomorrow" in UTC so the placeholder is always in the future.
    let dt = chrono::Utc::now() + chrono::Duration::days(1);
    dt.format("%Y-%m-%d").to_string()
}

impl EditState {
    /// Extract just the minute part of `cron_time` as a string for the Hourly
    /// picker. Stored as a full `HH:MM` so assemble_cron_expr can read it
    /// uniformly across all presets.
    fn cron_time_minute_only(&self) -> String {
        self.cron_time
            .split_once(':')
            .map(|(_, m)| m.to_string())
            .unwrap_or_else(|| "0".into())
    }
}

impl std::fmt::Display for CronPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CronPreset::Hourly => "Hourly",
            CronPreset::Daily => "Daily",
            CronPreset::Weekly => "Weekly",
            CronPreset::Monthly => "Monthly",
            CronPreset::Custom => "Custom",
        })
    }
}

#[derive(Default)]
struct HistoryState {
    runs: Vec<Run>,
    loaded_for: Option<String>,
    selected: Option<i64>,
    convo: Vec<(ChatMessage, Vec<ChatPart>)>,
    convo_session: Option<String>,
    convo_error: Option<String>,
    expanded: HashSet<String>,
}

struct App {
    rt: Handle,
    cli: Cli,
    db: Db,
    tasks: Vec<Task>,
    models: Vec<String>,
    selected_task: Option<String>,
    sub_tab: SubTab,
    edit: EditState,
    history: HistoryState,
    status: Option<StatusMessage>,
}

// =============================================================================
//                                MESSAGES
// =============================================================================

#[derive(Debug, Clone)]
enum Message {
    // sidebar
    TaskClicked(String),
    NewClicked,
    // tabs
    TabChanged(SubTab),
    // edit form
    NameChanged(String),
    KindChanged(ScheduleKind),
    // Once picker
    OnceDateChanged(String),
    OnceTimeChanged(String),
    // Cron picker
    CronPresetChanged(CronPreset),
    CronTimeChanged(String),
    CronDowChanged(String),
    CronDayChanged(String),
    CronRawChanged(String),
    WorkingDirChanged(String),
    BrowseClicked,
    BrowsePicked(Option<PathBuf>),
    ModelChanged(Option<String>),
    EnabledToggled(bool),
    SkipPermsToggled(bool),
    PromptEdit(text_editor::Action),
    // edit actions
    SaveClicked,
    RevertClicked,
    DeleteClicked,
    ConfirmDelete,
    CancelDelete,
    RunNowClicked,
    RunFinished,
    // history
    RefreshHistory,
    RunSelected(i64),
    ToggleExpanded(String),
    // status
    StatusTick,
}

// =============================================================================
//                                BOOT
// =============================================================================

struct Boot {
    rt: Handle,
    cli: Cli,
    db: Db,
    tasks: Vec<Task>,
    models: Vec<String>,
}

fn boot() -> anyhow::Result<Boot> {
    use anyhow::Context;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    let handle = rt.handle().clone();
    // Keep the runtime alive for the lifetime of the GUI. Iced's own event
    // loop drives the UI; the tokio runtime hosts scheduler + spawned runs.
    Box::leak(Box::new(rt));

    let cli = Cli::default();
    let models = handle
        .block_on(cli.list_models())
        .unwrap_or_else(|e| {
            tracing::warn!("failed to list models: {e:#}");
            Vec::new()
        })
        .into_iter()
        .map(|m: Model| m.combined())
        .collect();

    let db_path = PathBuf::from("data").join("runs.db");
    let db = Db::open(&db_path).context("opening db")?;

    let tasks_file = config::load(&config::tasks_file_path()).context("loading tasks.toml")?;
    let tasks = tasks_file.tasks.clone();

    // Bootstrap the scheduler exactly like the egui binary used to. Leak the
    // scheduler — it's a fire-and-forget background actor for the program's
    // lifetime, the UI never touches it after registration.
    handle.block_on(async {
        let repaint = Arc::new(Notify::new());
        let sched = Scheduler::new(cli.clone(), db.clone(), repaint).await?;
        for t in &tasks_file.tasks {
            sched.register(t.clone()).await?;
        }
        Box::leak(Box::new(sched));
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(Boot {
        rt: handle,
        cli,
        db,
        tasks,
        models,
    })
}

impl App {
    fn new(boot: Boot) -> (Self, iced::Task<Message>) {
        let selected_task = boot.tasks.first().map(|t| t.id.clone());
        let mut app = Self {
            rt: boot.rt,
            cli: boot.cli,
            db: boot.db,
            tasks: boot.tasks,
            models: boot.models,
            selected_task: selected_task.clone(),
            sub_tab: SubTab::Edit,
            edit: EditState::default(),
            history: HistoryState::default(),
            status: None,
        };
        if let Some(id) = &selected_task {
            app.load_edit_from_id(id);
        }
        (app, iced::Task::none())
    }

    fn load_edit_from_id(&mut self, task_id: &str) {
        let Some(t) = self.tasks.iter().find(|t| t.id == task_id) else { return };
        // Reset picker fields to their defaults so leftover state from a prior
        // task doesn't bleed in when the next task is Manual / Custom etc.
        let defaults = EditState::default();
        self.edit.once_date = defaults.once_date;
        self.edit.once_time = defaults.once_time;
        self.edit.cron_preset = defaults.cron_preset;
        self.edit.cron_time = defaults.cron_time;
        self.edit.cron_dow = defaults.cron_dow;
        self.edit.cron_day = defaults.cron_day;
        self.edit.cron_raw = defaults.cron_raw;

        self.edit.loaded_id = Some(t.id.clone());
        self.edit.dirty = false;
        self.edit.validation = None;
        self.edit.pending_delete = false;
        self.edit.f_name = t.name.clone();
        let (kind, expr) = split_schedule(&t.schedule);
        self.edit.f_kind = kind;
        match kind {
            ScheduleKind::Once => {
                if let Some((d, t)) = parse_once_expr(&expr) {
                    self.edit.once_date = d;
                    self.edit.once_time = t;
                }
            }
            ScheduleKind::Cron => {
                let parsed = parse_cron_expr(&expr);
                self.edit.cron_preset = parsed.preset;
                if let Some(t) = parsed.time { self.edit.cron_time = t; }
                if let Some(d) = parsed.dow { self.edit.cron_dow = d; }
                if let Some(d) = parsed.day { self.edit.cron_day = d; }
                self.edit.cron_raw = expr.clone();
            }
            ScheduleKind::Manual => {}
        }
        self.edit.f_working_dir = t.working_dir.to_string_lossy().into_owned();
        self.edit.f_model = if t.model.as_deref().map(str::is_empty).unwrap_or(true) {
            None
        } else {
            t.model.clone()
        };
        self.edit.f_skip_perms = t.dangerously_skip_permissions;
        self.edit.f_enabled = t.enabled;
        self.edit.prompt = text_editor::Content::with_text(&t.prompt);
    }

    fn current_task_name(&self) -> String {
        self.selected_task
            .as_ref()
            .and_then(|id| self.tasks.iter().find(|t| &t.id == id))
            .map(|t| t.name.clone())
            .unwrap_or_default()
    }

    fn assemble_schedule(&self) -> String {
        match self.edit.f_kind {
            ScheduleKind::Manual => "manual".into(),
            ScheduleKind::Cron => format!("cron:{}", self.assemble_cron_expr()),
            ScheduleKind::Once => format!("once:{}", self.assemble_once_expr()),
        }
    }

    fn assemble_once_expr(&self) -> String {
        // YYYY-MM-DD + HH:MM (UTC) -> RFC3339 with `Z`.
        // Build with chrono so we get strict validation; if the user typed
        // something invalid we still emit a string and let save's parse_schedule
        // surface the error.
        use chrono::{NaiveDate, NaiveTime, TimeZone};
        let d = NaiveDate::parse_from_str(self.edit.once_date.trim(), "%Y-%m-%d");
        let t = NaiveTime::parse_from_str(self.edit.once_time.trim(), "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(self.edit.once_time.trim(), "%H:%M:%S"));
        match (d, t) {
            (Ok(d), Ok(t)) => chrono::Utc
                .from_utc_datetime(&d.and_time(t))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            _ => format!(
                "{}T{}:00Z",
                self.edit.once_date.trim(),
                self.edit.once_time.trim()
            ),
        }
    }

    fn assemble_cron_expr(&self) -> String {
        use chrono::NaiveTime;
        let (h, m) = NaiveTime::parse_from_str(self.edit.cron_time.trim(), "%H:%M")
            .map(|t| (t.hour() as u8, t.minute() as u8))
            .unwrap_or((9, 0));
        match self.edit.cron_preset {
            // 6-field Quartz: sec min hour day mon dow
            CronPreset::Hourly => format!("0 {m} * * * *"),
            CronPreset::Daily => format!("0 {m} {h} * * *"),
            CronPreset::Weekly => format!("0 {m} {h} ? * {}", self.edit.cron_dow),
            CronPreset::Monthly => {
                let d: u8 = self.edit.cron_day.trim().parse().unwrap_or(1).clamp(1, 31);
                format!("0 {m} {h} {d} * *")
            }
            CronPreset::Custom => self.edit.cron_raw.trim().to_string(),
        }
    }

    fn candidate_task(&self, id: String) -> Task {
        Task {
            id,
            name: self.edit.f_name.trim().to_string(),
            schedule: self.assemble_schedule(),
            working_dir: PathBuf::from(self.edit.f_working_dir.trim()),
            model: self.edit.f_model.clone(),
            prompt: self.edit.prompt.text(),
            dangerously_skip_permissions: self.edit.f_skip_perms,
            enabled: self.edit.f_enabled,
        }
    }

    fn set_status(&mut self, kind: StatusKind, msg: impl Into<String>) {
        self.status = Some(StatusMessage {
            kind,
            text: msg.into(),
            at: Instant::now(),
        });
    }

    fn save_tasks_file(&self) -> anyhow::Result<()> {
        config::save(
            &config::tasks_file_path(),
            &TasksFile {
                tasks: self.tasks.clone(),
            },
        )
    }

    fn load_runs_for_selected(&mut self) {
        let Some(id) = self.selected_task.clone() else { return };
        let runs = self.db.list_recent_for_task(&id, 200).unwrap_or_default();
        self.history.runs = runs;
        self.history.loaded_for = Some(id);
    }

    fn load_convo_for(&mut self, run_id: i64) {
        let Some(run) = self.history.runs.iter().find(|r| r.id == run_id).cloned() else {
            return;
        };
        let Some(sid) = run.session_id else {
            self.history.convo.clear();
            self.history.convo_session = None;
            self.history.convo_error =
                Some("This run has no session id (opencode call may not have succeeded yet).".into());
            return;
        };
        if self.history.convo_session.as_deref() == Some(&sid) {
            return;
        }
        match storage::load_conversation(&sid) {
            Ok(c) => {
                self.history.convo = c;
                self.history.convo_session = Some(sid);
                self.history.convo_error = None;
                self.history.expanded.clear();
            }
            Err(e) => {
                self.history.convo.clear();
                self.history.convo_error = Some(format!("Failed to read storage: {e}"));
            }
        }
    }
}

fn split_schedule(s: &str) -> (ScheduleKind, String) {
    let s = s.trim();
    if s == "manual" || s.is_empty() {
        return (ScheduleKind::Manual, String::new());
    }
    if let Some(rest) = s.strip_prefix("cron:") {
        return (ScheduleKind::Cron, rest.trim().into());
    }
    if let Some(rest) = s.strip_prefix("once:") {
        return (ScheduleKind::Once, rest.trim().into());
    }
    (ScheduleKind::Manual, String::new())
}

/// Parse a stored `once:<RFC3339>` expression (no prefix) back into UI fields:
/// `YYYY-MM-DD` and `HH:MM`, both in UTC.
fn parse_once_expr(expr: &str) -> Option<(String, String)> {
    let dt = chrono::DateTime::parse_from_rfc3339(expr.trim()).ok()?;
    let utc = dt.with_timezone(&chrono::Utc);
    Some((
        utc.format("%Y-%m-%d").to_string(),
        utc.format("%H:%M").to_string(),
    ))
}

struct ParsedCron {
    preset: CronPreset,
    time: Option<String>,
    dow: Option<String>,
    day: Option<String>,
}

/// Heuristic-match a stored 6-field cron expression onto our preset enum.
/// Anything that doesn't match a clean pattern falls through as `Custom` so
/// the user can edit the raw expression.
fn parse_cron_expr(expr: &str) -> ParsedCron {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    let make_time = |h: &str, m: &str| -> Option<String> {
        let h: u8 = h.parse().ok()?;
        let m: u8 = m.parse().ok()?;
        if h < 24 && m < 60 {
            Some(format!("{h:02}:{m:02}"))
        } else {
            None
        }
    };

    if parts.len() == 6 && parts[0] == "0" {
        let (sec, min, hour, day, mon, dow) =
            (parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]);
        let _ = sec; // unused — already required to be "0"
        // Hourly:  0 <min> * * * *
        if hour == "*" && day == "*" && mon == "*" && dow == "*" {
            if let Some(t) = make_time("0", min) {
                return ParsedCron { preset: CronPreset::Hourly, time: Some(t), dow: None, day: None };
            }
        }
        // Daily:  0 <min> <hour> * * *
        if day == "*" && mon == "*" && dow == "*" {
            if let Some(t) = make_time(hour, min) {
                return ParsedCron { preset: CronPreset::Daily, time: Some(t), dow: None, day: None };
            }
        }
        // Weekly: 0 <min> <hour> ? * <DOW>
        if day == "?" && mon == "*" {
            let dow_upper = dow.to_ascii_uppercase();
            if DOW_OPTIONS.iter().any(|d| *d == dow_upper.as_str()) {
                if let Some(t) = make_time(hour, min) {
                    return ParsedCron {
                        preset: CronPreset::Weekly,
                        time: Some(t),
                        dow: Some(dow_upper),
                        day: None,
                    };
                }
            }
        }
        // Monthly: 0 <min> <hour> <day> * *
        if mon == "*" && dow == "*" {
            if let (Ok(d), Some(t)) = (day.parse::<u8>(), make_time(hour, min)) {
                if (1..=31).contains(&d) {
                    return ParsedCron {
                        preset: CronPreset::Monthly,
                        time: Some(t),
                        dow: None,
                        day: Some(d.to_string()),
                    };
                }
            }
        }
    }
    ParsedCron { preset: CronPreset::Custom, time: None, dow: None, day: None }
}

fn make_new_task(existing: &[Task]) -> Task {
    let mut n = 1;
    let name = loop {
        let candidate = if n == 1 {
            "New task".to_string()
        } else {
            format!("New task {n}")
        };
        if !existing.iter().any(|t| t.name == candidate) {
            break candidate;
        }
        n += 1;
    };
    Task {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        schedule: "manual".into(),
        working_dir: std::env::current_dir().unwrap_or_default(),
        model: None,
        prompt: String::new(),
        dangerously_skip_permissions: true,
        enabled: true,
    }
}

// =============================================================================
//                                UPDATE
// =============================================================================

const STATUS_LIFETIME_SECS: u64 = 6;

impl App {
    fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::TaskClicked(id) => {
                if self.selected_task.as_deref() != Some(&id) {
                    self.selected_task = Some(id.clone());
                    self.load_edit_from_id(&id);
                    self.history.selected = None;
                    self.history.loaded_for = None;
                    self.history.convo.clear();
                    self.history.convo_session = None;
                    self.history.convo_error = None;
                }
            }
            Message::NewClicked => {
                let task = make_new_task(&self.tasks);
                let id = task.id.clone();
                self.tasks.push(task);
                self.selected_task = Some(id.clone());
                self.sub_tab = SubTab::Edit;
                self.load_edit_from_id(&id);
                self.edit.dirty = true;
                self.set_status(StatusKind::Info, "New task created — Save to write to tasks.toml.");
            }
            Message::TabChanged(t) => {
                self.sub_tab = t;
                if t == SubTab::History {
                    self.load_runs_for_selected();
                }
            }
            Message::NameChanged(s) => { self.edit.f_name = s; self.edit.dirty = true; }
            Message::KindChanged(k) => {
                if self.edit.f_kind != k {
                    self.edit.f_kind = k;
                    self.edit.dirty = true;
                }
            }
            Message::OnceDateChanged(s) => { self.edit.once_date = s; self.edit.dirty = true; }
            Message::OnceTimeChanged(s) => { self.edit.once_time = s; self.edit.dirty = true; }
            Message::CronPresetChanged(p) => {
                if self.edit.cron_preset != p {
                    // When entering Custom for the first time, seed the raw
                    // field with the expression that would have been built
                    // from the previous preset so the user has something to
                    // edit instead of an empty string.
                    if p == CronPreset::Custom && self.edit.cron_raw.trim().is_empty() {
                        self.edit.cron_raw = self.assemble_cron_expr();
                    }
                    self.edit.cron_preset = p;
                    self.edit.dirty = true;
                }
            }
            Message::CronTimeChanged(s) => { self.edit.cron_time = s; self.edit.dirty = true; }
            Message::CronDowChanged(s) => { self.edit.cron_dow = s; self.edit.dirty = true; }
            Message::CronDayChanged(s) => {
                // Only accept digits, clamp to 1..=31 on input
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit()).take(2).collect();
                self.edit.cron_day = cleaned;
                self.edit.dirty = true;
            }
            Message::CronRawChanged(s) => { self.edit.cron_raw = s; self.edit.dirty = true; }
            Message::WorkingDirChanged(s) => { self.edit.f_working_dir = s; self.edit.dirty = true; }
            Message::ModelChanged(m) => { self.edit.f_model = m; self.edit.dirty = true; }
            Message::EnabledToggled(b) => { self.edit.f_enabled = b; self.edit.dirty = true; }
            Message::SkipPermsToggled(b) => { self.edit.f_skip_perms = b; self.edit.dirty = true; }
            Message::PromptEdit(action) => {
                let is_edit = matches!(action, text_editor::Action::Edit(_));
                self.edit.prompt.perform(action);
                if is_edit { self.edit.dirty = true; }
            }
            Message::BrowseClicked => {
                let start = if self.edit.f_working_dir.trim().is_empty() {
                    std::env::current_dir().unwrap_or_default()
                } else {
                    PathBuf::from(self.edit.f_working_dir.trim())
                };
                return iced::Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Pick working directory")
                            .set_directory(&start)
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::BrowsePicked,
                );
            }
            Message::BrowsePicked(Some(p)) => {
                self.edit.f_working_dir = p.to_string_lossy().into_owned();
                self.edit.dirty = true;
            }
            Message::BrowsePicked(None) => {}
            Message::SaveClicked => self.handle_save(),
            Message::RevertClicked => {
                if let Some(id) = self.selected_task.clone() {
                    self.load_edit_from_id(&id);
                    self.set_status(StatusKind::Info, "Reverted unsaved changes.");
                }
            }
            Message::DeleteClicked => { self.edit.pending_delete = true; }
            Message::CancelDelete => { self.edit.pending_delete = false; }
            Message::ConfirmDelete => self.handle_delete(),
            Message::RunNowClicked => return self.handle_run_now(),
            Message::RunFinished => self.load_runs_for_selected(),
            Message::RefreshHistory => self.load_runs_for_selected(),
            Message::RunSelected(id) => {
                self.history.selected = Some(id);
                self.load_convo_for(id);
            }
            Message::ToggleExpanded(id) => {
                if !self.history.expanded.remove(&id) {
                    self.history.expanded.insert(id);
                }
            }
            Message::StatusTick => {
                if let Some(s) = &self.status {
                    if s.at.elapsed().as_secs() >= STATUS_LIFETIME_SECS {
                        self.status = None;
                    }
                }
            }
        }
        iced::Task::none()
    }

    fn handle_save(&mut self) {
        let Some(id) = self.selected_task.clone() else { return };
        let candidate = self.candidate_task(id.clone());
        if candidate.name.is_empty() {
            self.edit.validation = Some("Name is required.".into());
            return;
        }
        if let Err(e) = candidate.parse_schedule() {
            self.edit.validation = Some(format!("Invalid schedule: {e:#}"));
            return;
        }
        if let Some(idx) = self.tasks.iter().position(|t| t.id == id) {
            self.tasks[idx] = candidate;
        } else {
            self.tasks.push(candidate);
        }
        match self.save_tasks_file() {
            Ok(_) => {
                self.edit.dirty = false;
                self.edit.validation = None;
                self.set_status(
                    StatusKind::Success,
                    "Saved to tasks.toml. Restart to apply scheduling changes.",
                );
            }
            Err(e) => {
                self.edit.validation = Some(format!("Failed to write tasks.toml: {e:#}"));
            }
        }
    }

    fn handle_delete(&mut self) {
        let Some(id) = self.selected_task.clone() else { return };
        self.tasks.retain(|t| t.id != id);
        if let Err(e) = self.save_tasks_file() {
            self.set_status(StatusKind::Error, format!("Delete failed to persist: {e:#}"));
            return;
        }
        self.selected_task = self.tasks.first().map(|t| t.id.clone());
        if let Some(new_id) = self.selected_task.clone() {
            self.load_edit_from_id(&new_id);
        } else {
            self.edit = EditState::default();
        }
        self.set_status(StatusKind::Info, "Task deleted. Restart to drop its schedule.");
    }

    fn handle_run_now(&mut self) -> iced::Task<Message> {
        if self.edit.dirty {
            self.handle_save();
            if self.edit.validation.is_some() { return iced::Task::none(); }
        }
        let Some(id) = self.selected_task.clone() else { return iced::Task::none() };
        let Some(task) = self.tasks.iter().find(|t| t.id == id).cloned() else {
            return iced::Task::none();
        };
        self.set_status(StatusKind::Info, format!("Triggered `{}`…", task.name));

        let cli = self.cli.clone();
        let db = self.db.clone();
        let rt = self.rt.clone();
        iced::Task::perform(
            async move {
                let _ = rt
                    .spawn(async move { runner::execute(&task, &cli, &db).await })
                    .await;
            },
            |_| Message::RunFinished,
        )
    }
}

// =============================================================================
//                                VIEW
// =============================================================================

impl App {
    fn view(&self) -> Element<Message> {
        column![
            row![self.sidebar(), self.center()].height(Length::Fill),
            self.status_bar(),
        ]
        .into()
    }

    fn sidebar(&self) -> Element<Message> {
        let header = row![
            text("Tasks").size(15).style(|_| text::Style { color: Some(TEXT_C) }).font(bold()),
            Space::with_width(Length::Fill),
            primary_icon_button(ICON_PLUS, "New").on_press(Message::NewClicked),
        ]
        .align_y(Vertical::Center);

        let count = text(format!("{} configured", self.tasks.len()))
            .size(12)
            .style(|_| text::Style { color: Some(TEXT_FAINT) });

        let body: Element<_> = if self.tasks.is_empty() {
            sidebar_empty().into()
        } else {
            let rows = self.tasks.iter().fold(column![].spacing(2), |col, t| {
                let selected = self.selected_task.as_deref() == Some(&t.id);
                let dirty = selected && self.edit.dirty;
                col.push(task_row(t, selected, dirty))
            });
            scrollable(rows).height(Length::Fill).into()
        };

        container(column![
            header,
            Space::with_height(4),
            count,
            Space::with_height(14),
            body,
        ])
        .padding(Padding { left: 16.0, right: 16.0, top: PAGE_PAD_Y, bottom: 8.0 })
        .width(Length::Fixed(260.0))
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(BG)),
            border: Border { color: BORDER_C, width: 1.0, radius: Radius::new(0.0) },
            ..Default::default()
        })
        .into()
    }

    fn center(&self) -> Element<Message> {
        if self.selected_task.is_none() {
            return container(empty_center())
                .padding(Padding::new(0.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(BG)),
                    ..Default::default()
                })
                .into();
        }

        let heading = text(self.current_task_name())
            .size(22)
            .style(|_| text::Style { color: Some(TEXT_C) })
            .font(bold());

        let body: Element<_> = match self.sub_tab {
            SubTab::Edit => self.edit_view(),
            SubTab::History => self.history_view(),
        };

        container(column![
            heading,
            Space::with_height(14),
            self.tab_bar(),
            Space::with_height(18),
            body,
        ])
        .padding(Padding { left: PAGE_PAD_X, right: PAGE_PAD_X, top: PAGE_PAD_Y, bottom: 0.0 })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(BG)),
            ..Default::default()
        })
        .into()
    }

    fn tab_bar(&self) -> Element<Message> {
        let tab = |label: &'static str, this: SubTab| {
            let selected = self.sub_tab == this;
            let color = if selected { TEXT_C } else { TEXT_MUTED };
            let underline_color = if selected { ACCENT } else { Color::TRANSPARENT };
            let body = column![
                text(label)
                    .size(14)
                    .style(move |_| text::Style { color: Some(color) })
                    .font(if selected { bold() } else { Font::DEFAULT }),
                Space::with_height(6),
                container(Space::with_height(2)).width(Length::Fill).style(move |_| {
                    container::Style {
                        background: Some(Background::Color(underline_color)),
                        ..Default::default()
                    }
                }),
            ]
            .align_x(Horizontal::Center);

            let inner = container(body).padding(Padding {
                left: 14.0, right: 14.0, top: 4.0, bottom: 0.0,
            });
            mouse_area(inner)
                .on_press(Message::TabChanged(this))
                .interaction(iced::mouse::Interaction::Pointer)
        };

        column![
            row![tab("Edit", SubTab::Edit), tab("History", SubTab::History)].spacing(0),
            container(Space::with_height(1))
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(BORDER_C)),
                    ..Default::default()
                }),
        ]
        .into()
    }

    // ----- edit tab -----

    fn edit_view(&self) -> Element<Message> {
        let action_bar = row![
            primary_icon_button(ICON_SAVE, "Save").on_press(Message::SaveClicked),
            if self.edit.dirty {
                Element::from(
                    icon_button(ICON_REVERT, "Revert", TEXT_MUTED).on_press(Message::RevertClicked),
                )
            } else {
                Element::from(Space::with_width(0))
            },
            Space::with_width(6),
            icon_button(ICON_PLAY, "Run now", SUCCESS).on_press(Message::RunNowClicked),
            Space::with_width(Length::Fill),
            danger_icon_button(ICON_TRASH, "Delete").on_press(Message::DeleteClicked),
        ]
        .align_y(Vertical::Center)
        .spacing(8);

        let alert: Element<_> = if let Some(e) = &self.edit.validation {
            inline_alert(StatusKind::Error, e).into()
        } else if self.edit.dirty {
            inline_alert(StatusKind::Warn, "Unsaved changes").into()
        } else {
            Space::with_height(0).into()
        };

        let confirm: Element<_> = if self.edit.pending_delete {
            confirm_delete_bar().into()
        } else {
            Space::with_height(0).into()
        };

        let basics = column![
            section("Basics"),
            form_label("Name"),
            text_input("A short label for this task", &self.edit.f_name)
                .on_input(Message::NameChanged)
                .padding(8)
                .style(text_input_style),
            Space::with_height(10),
            checkbox(
                if self.edit.f_enabled { "Scheduler runs this task" } else { "Paused — won't run on its schedule" },
                self.edit.f_enabled,
            )
            .on_toggle(Message::EnabledToggled)
            .style(checkbox_style),
        ];

        let schedule = column![
            Space::with_height(20),
            section("Schedule"),
            schedule_kind_picker(self.edit.f_kind),
            Space::with_height(10),
            schedule_body(&self.edit),
        ];

        let exec = column![
            Space::with_height(20),
            section("Execution"),
            form_label("Working dir"),
            row![
                text_input("/path/to/project", &self.edit.f_working_dir)
                    .on_input(Message::WorkingDirChanged)
                    .padding(8)
                    .style(text_input_style),
                Space::with_width(8),
                icon_button(ICON_FOLDER, "Browse", TEXT_C).on_press(Message::BrowseClicked),
            ]
            .align_y(Vertical::Center),
            Space::with_height(10),
            form_label("Model"),
            pick_list(
                {
                    let mut opts = vec!["(opencode default)".to_string()];
                    opts.extend(self.models.iter().cloned());
                    opts
                },
                Some(
                    self.edit.f_model.clone().unwrap_or_else(|| "(opencode default)".to_string()),
                ),
                |s| {
                    Message::ModelChanged(if s == "(opencode default)" { None } else { Some(s) })
                },
            )
            .width(Length::Fill)
            .padding(8)
            .style(pick_list_style),
            Space::with_height(10),
            checkbox(
                if self.edit.f_skip_perms { "Skip permission prompts (dangerous)" } else { "Ask before sensitive actions" },
                self.edit.f_skip_perms,
            )
            .on_toggle(Message::SkipPermsToggled)
            .style(checkbox_style),
            perms_warning(self.edit.f_skip_perms),
        ];

        let prompt_block = column![
            Space::with_height(20),
            section("Prompt"),
            container(
                text_editor(&self.edit.prompt)
                    .on_action(Message::PromptEdit)
                    .height(Length::Fixed(220.0))
                    .padding(10),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(SURFACE_2)),
                border: Border {
                    color: BORDER_C,
                    width: 1.0,
                    radius: Radius::new(RADIUS_SM),
                },
                ..Default::default()
            }),
            Space::with_height(4),
            text(format!(
                "{} line(s) · {} char(s)",
                self.edit.prompt.line_count(),
                self.edit.prompt.text().chars().count()
            ))
            .size(11)
            .style(|_| text::Style { color: Some(TEXT_FAINT) }),
        ];

        scrollable(
            column![
                action_bar,
                Space::with_height(10),
                confirm,
                alert,
                Space::with_height(14),
                basics,
                schedule,
                exec,
                prompt_block,
                Space::with_height(24),
            ]
            .padding(Padding { left: 0.0, right: 8.0, top: 0.0, bottom: 0.0 }),
        )
        .height(Length::Fill)
        .into()
    }

    // ----- history tab -----

    fn history_view(&self) -> Element<Message> {
        let counts = run_counts(&self.history.runs);

        let header = row![
            icon_button(ICON_REFRESH, "Refresh", TEXT_C).on_press(Message::RefreshHistory),
            Space::with_width(8),
            text(format!("{} total", counts.total))
                .size(13)
                .style(|_| text::Style { color: Some(TEXT_MUTED) }),
        ]
        .push_maybe((counts.running > 0).then(|| chip(format!("{} running", counts.running), INFO)))
        .push_maybe((counts.ok > 0).then(|| chip(format!("{} ok", counts.ok), SUCCESS)))
        .push_maybe((counts.err > 0).then(|| chip(format!("{} err", counts.err), ERROR)))
        .spacing(8)
        .align_y(Vertical::Center);

        // left run list
        let runs: Element<_> = if self.history.runs.is_empty() {
            empty_runs().into()
        } else {
            let col = self.history.runs.iter().fold(column![].spacing(4), |c, r| {
                c.push(run_row(r, self.history.selected == Some(r.id)))
            });
            scrollable(col).height(Length::Fill).into()
        };

        // right convo
        let convo: Element<_> = if let Some(id) = self.history.selected {
            if let Some(run) = self.history.runs.iter().find(|r| r.id == id).cloned() {
                self.conversation_view(run)
            } else {
                empty_convo().into()
            }
        } else {
            empty_convo().into()
        };

        column![
            header,
            Space::with_height(14),
            row![
                container(runs)
                    .width(Length::Fixed(320.0))
                    .height(Length::Fill),
                Space::with_width(16),
                container(Space::with_width(Length::Fixed(1.0)))
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Background::Color(BORDER_C)),
                        ..Default::default()
                    }),
                Space::with_width(20),
                container(scrollable(convo))
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .height(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    fn conversation_view(&self, run: Run) -> Element<Message> {
        let mut col = column![].spacing(0);

        // Run meta header
        let (status_color, status_label) = run_status_style(&run.status);
        let mut header = column![
            row![
                text(format!("Run #{}", run.id))
                    .size(16)
                    .style(|_| text::Style { color: Some(TEXT_C) })
                    .font(bold()),
                Space::with_width(8),
                chip(status_label, status_color),
            ]
            .align_y(Vertical::Center),
            Space::with_height(6),
            meta_row("Started", &run.started_at.format("%Y-%m-%d %H:%M:%S").to_string()),
        ];
        if let Some(f) = run.finished_at {
            header = header.push(meta_row("Finished", &f.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        if let Some(sid) = &run.session_id {
            header = header.push(meta_row("Session", sid));
        }
        col = col.push(header);

        if let Some(err) = &run.error {
            col = col.push(Space::with_height(8));
            col = col.push(error_card(err));
        }

        col = col.push(Space::with_height(10));
        col = col.push(
            container(Space::with_height(Length::Fixed(1.0)))
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(BORDER_C)),
                    ..Default::default()
                }),
        );
        col = col.push(Space::with_height(14));

        if let Some(e) = &self.history.convo_error {
            col = col.push(error_card(e));
            return col.into();
        }
        if self.history.convo.is_empty() {
            col = col.push(
                text(
                    "No messages yet — opencode may still be running, or this session has no on-disk data.",
                )
                .size(13)
                .style(|_| text::Style { color: Some(TEXT_MUTED) }),
            );
            return col.into();
        }

        for (msg, parts) in &self.history.convo {
            col = col.push(message_card(msg, parts, &self.history.expanded));
            col = col.push(Space::with_height(10));
        }
        col.into()
    }

    // ----- status bar -----

    fn status_bar(&self) -> Element<Message> {
        let summary = text(format!(
            "{} task(s) · {} models · scheduling refreshes on restart",
            self.tasks.len(),
            self.models.len(),
        ))
        .size(12)
        .style(|_| text::Style { color: Some(TEXT_FAINT) });

        let msg: Element<_> = if let Some(s) = &self.status {
            let c = s.kind.color();
            row![
                icon_svg(s.kind.icon_bytes(), 13.0, c),
                Space::with_width(6),
                text(s.text.clone()).size(12).style(move |_| text::Style { color: Some(c) }),
            ]
            .align_y(Vertical::Center)
            .into()
        } else {
            Space::with_width(0).into()
        };

        container(row![summary, Space::with_width(Length::Fill), msg].align_y(Vertical::Center))
            .padding(Padding { left: PAGE_PAD_X, right: PAGE_PAD_X, top: 10.0, bottom: 10.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(BG)),
                border: Border { color: BORDER_C, width: 1.0, radius: Radius::new(0.0) },
                ..Default::default()
            })
            .into()
    }
}

// =============================================================================
//                                COMPONENTS
// =============================================================================

fn bold() -> Font {
    Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT }
}

fn sidebar_empty<'a>() -> Element<'a, Message> {
    column![
        Space::with_height(20),
        text("No tasks yet").size(13).style(|_| text::Style { color: Some(TEXT_MUTED) }),
        Space::with_height(4),
        text("Click + New to add one.").size(12).style(|_| text::Style { color: Some(TEXT_FAINT) }),
    ]
    .align_x(Horizontal::Center)
    .width(Length::Fill)
    .into()
}

fn empty_center<'a>() -> Element<'a, Message> {
    column![
        Space::with_height(120),
        text("Nothing selected").size(18).style(|_| text::Style { color: Some(TEXT_MUTED) }).font(bold()),
        Space::with_height(6),
        text("Pick a task on the left, or click + New to create one.")
            .size(14)
            .style(|_| text::Style { color: Some(TEXT_FAINT) }),
    ]
    .align_x(Horizontal::Center)
    .width(Length::Fill)
    .into()
}

fn empty_runs<'a>() -> Element<'a, Message> {
    column![
        Space::with_height(40),
        text("No runs yet").size(14).style(|_| text::Style { color: Some(TEXT_MUTED) }),
        Space::with_height(4),
        text("Click Run now on the Edit tab.").size(12).style(|_| text::Style { color: Some(TEXT_FAINT) }),
    ]
    .align_x(Horizontal::Center)
    .width(Length::Fill)
    .into()
}

fn empty_convo<'a>() -> Element<'a, Message> {
    column![
        Space::with_height(80),
        text("No run selected").size(16).style(|_| text::Style { color: Some(TEXT_MUTED) }).font(bold()),
        Space::with_height(4),
        text("Pick a run on the left to view its conversation.")
            .size(13)
            .style(|_| text::Style { color: Some(TEXT_FAINT) }),
    ]
    .align_x(Horizontal::Center)
    .width(Length::Fill)
    .into()
}

fn task_row<'a>(t: &'a Task, selected: bool, dirty: bool) -> Element<'a, Message> {
    let dot_color = if t.enabled { SUCCESS } else { TEXT_FAINT };
    let dot = container(Space::new(Length::Fixed(8.0), Length::Fixed(8.0))).style(move |_| {
        container::Style {
            background: Some(Background::Color(dot_color)),
            border: Border { radius: Radius::new(4.0), ..Default::default() },
            ..Default::default()
        }
    });

    let (chip_label, chip_color) = schedule_chip(&t.schedule);
    let chips = row![chip(chip_label, chip_color)]
        .push_maybe((!t.enabled).then(|| chip("paused", TEXT_FAINT)))
        .spacing(6);

    let name_color = if t.enabled { TEXT_C } else { TEXT_MUTED };
    let name = text(t.name.clone())
        .size(14)
        .style(move |_| text::Style { color: Some(name_color) })
        .font(Font { weight: iced::font::Weight::Semibold, ..Font::DEFAULT });

    let dirty_dot: Element<_> = if dirty {
        container(Space::new(Length::Fixed(8.0), Length::Fixed(8.0)))
            .style(|_| container::Style {
                background: Some(Background::Color(WARN)),
                border: Border { radius: Radius::new(4.0), ..Default::default() },
                ..Default::default()
            })
            .into()
    } else {
        Space::with_width(0).into()
    };

    let body = column![
        row![dot, name, Space::with_width(Length::Fill), dirty_dot].spacing(10).align_y(Vertical::Center),
        Space::with_height(3),
        chips,
    ];

    let inner = container(body)
        .padding(Padding { left: 12.0, right: 10.0, top: 8.0, bottom: 8.0 })
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(if selected { SURFACE } else { Color::TRANSPARENT })),
            border: Border {
                color: if selected { ACCENT } else { Color::TRANSPARENT },
                width: if selected { 1.0 } else { 0.0 },
                radius: Radius::new(RADIUS_SM),
            },
            ..Default::default()
        });

    let id = t.id.clone();
    mouse_area(inner)
        .on_press(Message::TaskClicked(id))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

// ----- history bits -----

struct Counts { total: usize, running: usize, ok: usize, err: usize }
fn run_counts(runs: &[Run]) -> Counts {
    let mut c = Counts { total: runs.len(), running: 0, ok: 0, err: 0 };
    for r in runs {
        match r.status.as_str() {
            "running" => c.running += 1,
            "ok" => c.ok += 1,
            "error" => c.err += 1,
            _ => {}
        }
    }
    c
}

fn run_status_style(s: &str) -> (Color, &'static str) {
    match s {
        "ok" => (SUCCESS, "ok"),
        "error" => (ERROR, "error"),
        "running" => (INFO, "running"),
        _ => (TEXT_FAINT, "?"),
    }
}

fn format_when(t: DateTime<Utc>) -> String {
    let local = t.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    let same_day = local.date_naive() == now.date_naive();
    if same_day { local.format("Today %H:%M:%S").to_string() }
    else if (now - local).num_days() < 7 { local.format("%a %H:%M").to_string() }
    else { local.format("%Y-%m-%d %H:%M").to_string() }
}

fn duration_label(r: &Run) -> Option<String> {
    let f = r.finished_at?;
    let secs = (f - r.started_at).num_seconds().max(0);
    Some(if secs < 60 { format!("{secs}s") }
        else if secs < 3600 { format!("{}m {}s", secs / 60, secs % 60) }
        else { format!("{}h {}m", secs / 3600, (secs % 3600) / 60) })
}

fn run_row(r: &Run, selected: bool) -> Element<Message> {
    let (color, label) = run_status_style(&r.status);
    let header = row![
        text(format!("#{}", r.id))
            .size(14)
            .style(|_| text::Style { color: Some(TEXT_C) })
            .font(Font {
                family: iced::font::Family::Monospace,
                weight: iced::font::Weight::Bold,
                ..Font::DEFAULT
            }),
        Space::with_width(Length::Fill),
        chip(label, color),
    ]
    .align_y(Vertical::Center);

    let when = text(format_when(r.started_at))
        .size(12)
        .style(|_| text::Style { color: Some(TEXT_MUTED) });
    let mut meta_row_w = row![when].spacing(6).align_y(Vertical::Center);
    if let Some(d) = duration_label(r) {
        meta_row_w = meta_row_w.push(
            text(format!("· {d}"))
                .size(12)
                .style(|_| text::Style { color: Some(TEXT_FAINT) }),
        );
    }

    let body = column![header, Space::with_height(3), meta_row_w];
    let inner = container(body)
        .padding(Padding { left: 12.0, right: 12.0, top: 9.0, bottom: 9.0 })
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(if selected { SURFACE } else { Color::TRANSPARENT })),
            border: Border {
                color: if selected { ACCENT } else { BORDER_C },
                width: 1.0,
                radius: Radius::new(RADIUS_SM),
            },
            ..Default::default()
        });

    let id = r.id;
    mouse_area(inner)
        .on_press(Message::RunSelected(id))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn meta_row<'a>(label: &str, value: &str) -> Element<'a, Message> {
    row![
        text(label.to_string()).size(12).style(|_| text::Style { color: Some(TEXT_FAINT) }),
        Space::with_width(12),
        text(value.to_string())
            .size(12)
            .style(|_| text::Style { color: Some(TEXT_MUTED) })
            .font(Font { family: iced::font::Family::Monospace, ..Font::DEFAULT }),
    ]
    .align_y(Vertical::Center)
    .into()
}

fn error_card<'a>(msg: &str) -> Element<'a, Message> {
    container(row![
        icon_svg(ICON_X, 13.0, ERROR),
        Space::with_width(8),
        text(msg.to_string()).size(13).style(|_| text::Style { color: Some(TEXT_C) }),
    ].align_y(Vertical::Center))
    .padding(Padding { left: 10.0, right: 10.0, top: 8.0, bottom: 8.0 })
    .style(|_| container::Style {
        background: Some(Background::Color(SURFACE_2)),
        border: Border { color: rgba(ERROR, 0.4), width: 1.0, radius: Radius::new(RADIUS_SM) },
        ..Default::default()
    })
    .into()
}

// ----- conversation rendering -----

fn role_color(role: &str) -> Color {
    match role {
        "user" => INFO,
        "assistant" => SUCCESS,
        "system" => WARN,
        "tool" => ACCENT,
        _ => TEXT_FAINT,
    }
}

fn message_card<'a>(
    msg: &'a ChatMessage,
    parts: &'a [ChatPart],
    expanded: &'a HashSet<String>,
) -> Element<'a, Message> {
    let role = msg.role.as_deref().unwrap_or("?");
    let color = role_color(role);

    let mut col = column![row![chip(role, color)].spacing(6), Space::with_height(8)];

    for p in parts {
        col = col.push(render_part(p, expanded));
        col = col.push(Space::with_height(6));
    }

    container(col)
        .padding(Padding { left: 14.0, right: 14.0, top: 12.0, bottom: 12.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(SURFACE_2)),
            border: Border { color: BORDER_C, width: 1.0, radius: Radius::new(RADIUS) },
            ..Default::default()
        })
        .into()
}

fn render_part<'a>(part: &'a ChatPart, expanded: &'a HashSet<String>) -> Element<'a, Message> {
    let kind = part.kind.as_deref().unwrap_or("");
    match kind {
        "text" | "" => {
            let body = part.text.as_deref().or_else(|| {
                part.extra.get("text").and_then(|v| v.as_str())
            });
            match body {
                Some(t) => text(t.to_string())
                    .size(13)
                    .style(|_| text::Style { color: Some(TEXT_C) })
                    .into(),
                None => Space::with_height(0).into(),
            }
        }
        "reasoning" => render_reasoning(part, expanded),
        "step-start" => step_divider("step start"),
        "step-finish" => render_step_finish(part),
        "patch" => render_patch(part, expanded),
        "tool" | "tool_call" | "tool-invocation" => render_tool(part, expanded),
        other => {
            let id = format!("other-{}-{}", other, part.id);
            collapsible(
                &id,
                row![
                    text(format!("[{other}]"))
                        .size(12)
                        .style(|_| text::Style { color: Some(TEXT_MUTED) }),
                ]
                .into(),
                expanded,
                || code_block(&pretty_extra(part)),
            )
        }
    }
}

fn render_reasoning<'a>(
    part: &'a ChatPart,
    expanded: &'a HashSet<String>,
) -> Element<'a, Message> {
    let text_body = part.text.as_deref().or_else(|| {
        part.extra.get("text").and_then(|v| v.as_str())
    });
    let Some(t) = text_body else { return Space::with_height(0).into() };
    let t = t.to_string();
    let id = format!("reasoning-{}", part.id);
    collapsible(
        &id,
        row![
            text("Reasoning")
                .size(11)
                .style(|_| text::Style { color: Some(TEXT_FAINT) })
                .font(Font {
                    style: iced::font::Style::Italic,
                    ..Font::DEFAULT
                }),
        ]
        .into(),
        expanded,
        move || {
            text(t.clone())
                .size(12)
                .style(|_| text::Style { color: Some(TEXT_MUTED) })
                .font(Font { style: iced::font::Style::Italic, ..Font::DEFAULT })
                .into()
        },
    )
}

fn render_tool<'a>(
    part: &'a ChatPart,
    expanded: &'a HashSet<String>,
) -> Element<'a, Message> {
    let name = part
        .extra
        .get("tool")
        .or_else(|| part.extra.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();

    let state = part.extra.get("state");
    let status = state.and_then(|s| s.get("status")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let status_color = match status.as_str() {
        "completed" => SUCCESS,
        "error" => ERROR,
        "running" | "pending" => INFO,
        _ => TEXT_FAINT,
    };

    let input = state.and_then(|s| s.get("input")).cloned();
    let output = state.and_then(|s| s.get("output")).cloned();

    let id = format!("tool-{}", part.id);
    let header_label = name.clone();
    let status_label = status.clone();
    collapsible(
        &id,
        row![
            icon_svg(ICON_WRENCH, 13.0, ACCENT),
            Space::with_width(6),
            text(header_label)
                .size(13)
                .style(|_| text::Style { color: Some(ACCENT_TEXT) })
                .font(bold()),
            Space::with_width(8),
            if status_label.is_empty() {
                Element::from(Space::with_width(0))
            } else {
                chip(status_label, status_color)
            },
        ]
        .align_y(Vertical::Center)
        .into(),
        expanded,
        move || {
            let mut col = column![].spacing(6);
            if let Some(v) = &input {
                col = col.push(
                    text("INPUT")
                        .size(10)
                        .style(|_| text::Style { color: Some(TEXT_FAINT) })
                        .font(bold()),
                );
                col = col.push(code_block(&pretty_value(v)));
            }
            if let Some(v) = &output {
                col = col.push(
                    text("OUTPUT")
                        .size(10)
                        .style(|_| text::Style { color: Some(TEXT_FAINT) })
                        .font(bold()),
                );
                let rendered = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => pretty_value(other),
                };
                let trimmed = if rendered.chars().count() > 8000 {
                    let mut s: String = rendered.chars().take(8000).collect();
                    s.push_str("\n… (truncated)");
                    s
                } else {
                    rendered
                };
                col = col.push(code_block(&trimmed));
            }
            col.into()
        },
    )
}

fn render_step_finish<'a>(part: &'a ChatPart) -> Element<'a, Message> {
    let reason = part
        .extra
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("step finished")
        .to_string();
    let tokens = part.extra.get("tokens");
    let total = tokens.and_then(|t| t.get("total")).and_then(|v| v.as_i64());
    let inp = tokens.and_then(|t| t.get("input")).and_then(|v| v.as_i64());
    let out = tokens.and_then(|t| t.get("output")).and_then(|v| v.as_i64());

    let mut r = row![chip(reason, TEXT_MUTED)].spacing(6).align_y(Vertical::Center);
    if let (Some(t), Some(i), Some(o)) = (total, inp, out) {
        r = r.push(
            text(format!("· {t} tok ({i} in / {o} out)"))
                .size(11)
                .style(|_| text::Style { color: Some(TEXT_FAINT) }),
        );
    } else if let Some(t) = total {
        r = r.push(
            text(format!("· {t} tok"))
                .size(11)
                .style(|_| text::Style { color: Some(TEXT_FAINT) }),
        );
    }
    r.into()
}

fn render_patch<'a>(
    part: &'a ChatPart,
    expanded: &'a HashSet<String>,
) -> Element<'a, Message> {
    let files: Vec<String> = part
        .extra
        .get("files")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let hash = part
        .extra
        .get("hash")
        .and_then(|v| v.as_str())
        .map(|s| s[..s.len().min(8)].to_string())
        .unwrap_or_default();

    let header_label = if files.is_empty() {
        format!("Patch {hash}")
    } else {
        format!("Patch {hash} · {} file(s)", files.len())
    };

    let id = format!("patch-{}", part.id);
    let files_clone = files.clone();
    collapsible(
        &id,
        row![
            text(header_label)
                .size(13)
                .style(|_| text::Style { color: Some(ACCENT_TEXT) })
                .font(bold()),
        ]
        .into(),
        expanded,
        move || {
            files_clone
                .iter()
                .fold(column![].spacing(2), |c, f| {
                    c.push(
                        text(f.clone())
                            .size(12)
                            .style(|_| text::Style { color: Some(TEXT_MUTED) })
                            .font(Font { family: iced::font::Family::Monospace, ..Font::DEFAULT }),
                    )
                })
                .into()
        },
    )
}

fn step_divider<'a>(label: &str) -> Element<'a, Message> {
    let lbl = label.to_string();
    row![
        container(Space::with_height(Length::Fixed(1.0)))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(BORDER_C)),
                ..Default::default()
            }),
        Space::with_width(8),
        text(lbl).size(11).style(|_| text::Style { color: Some(TEXT_FAINT) }),
        Space::with_width(8),
        container(Space::with_height(Length::Fixed(1.0)))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(BORDER_C)),
                ..Default::default()
            }),
    ]
    .align_y(Vertical::Center)
    .into()
}

fn collapsible<'a, F>(
    id: &str,
    header: Element<'a, Message>,
    expanded: &'a HashSet<String>,
    body_fn: F,
) -> Element<'a, Message>
where
    F: FnOnce() -> Element<'a, Message>,
{
    let is_open = expanded.contains(id);
    let id_owned = id.to_string();
    let chevron = text(if is_open { "▾" } else { "▸" })
        .size(10)
        .style(|_| text::Style { color: Some(TEXT_FAINT) });

    let head = mouse_area(
        row![chevron, Space::with_width(4), header]
            .align_y(Vertical::Center),
    )
    .on_press(Message::ToggleExpanded(id_owned))
    .interaction(iced::mouse::Interaction::Pointer);

    let body: Element<_> = if is_open {
        column![Space::with_height(4), body_fn()].into()
    } else {
        Space::with_height(0).into()
    };

    column![head, body].into()
}

fn code_block<'a>(content: &str) -> Element<'a, Message> {
    let body = content.to_string();
    container(
        text(body)
            .size(12)
            .style(|_| text::Style { color: Some(TEXT_MUTED) })
            .font(Font { family: iced::font::Family::Monospace, ..Font::DEFAULT }),
    )
    .padding(8)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(BG)),
        border: Border { color: BORDER_C, width: 1.0, radius: Radius::new(RADIUS_SM) },
        ..Default::default()
    })
    .into()
}

fn pretty_extra(part: &ChatPart) -> String {
    serde_json::to_string_pretty(&part.extra).unwrap_or_else(|_| "<unprintable>".into())
}
fn pretty_value(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| "<unprintable>".into())
}

// ----- shared bits -----

fn schedule_chip(schedule: &str) -> (&'static str, Color) {
    if schedule == "manual" { ("manual", TEXT_MUTED) }
    else if schedule.starts_with("cron:") { ("cron", INFO) }
    else if schedule.starts_with("once:") { ("once", WARN) }
    else { ("?", ERROR) }
}

fn chip<'a>(label: impl Into<String>, accent: Color) -> Element<'a, Message> {
    container(
        text(label.into())
            .size(11)
            .style(move |_| text::Style { color: Some(accent) })
            .font(bold()),
    )
    .padding(Padding { left: 7.0, right: 7.0, top: 1.0, bottom: 1.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(SURFACE_2)),
        border: Border {
            color: rgba(accent, 0.35),
            width: 1.0,
            radius: Radius::new(999.0),
        },
        ..Default::default()
    })
    .into()
}

fn section<'a>(label: &'static str) -> Element<'a, Message> {
    column![
        text(label.to_uppercase()).size(11).style(|_| text::Style { color: Some(TEXT_FAINT) }).font(bold()),
        Space::with_height(4),
        container(Space::with_height(1)).width(Length::Fill).style(|_| container::Style {
            background: Some(Background::Color(BORDER_C)),
            ..Default::default()
        }),
        Space::with_height(12),
    ]
    .into()
}

fn form_label<'a>(label: &'static str) -> Element<'a, Message> {
    column![
        text(label).size(12).style(|_| text::Style { color: Some(TEXT_MUTED) }),
        Space::with_height(3),
    ]
    .into()
}

fn schedule_kind_picker(current: ScheduleKind) -> Element<'static, Message> {
    let opt = |kind: ScheduleKind, label: &'static str, icon_bytes: &'static [u8]| {
        let selected = current == kind;
        let color = if selected { ACCENT } else { TEXT_MUTED };
        let body = row![
            icon_svg(icon_bytes, 14.0, color),
            Space::with_width(6),
            text(label).size(13).style(move |_| text::Style { color: Some(color) }),
        ]
        .align_y(Vertical::Center);
        button(body)
            .padding(Padding { left: 10.0, right: 12.0, top: 6.0, bottom: 6.0 })
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if selected { SURFACE } else { SURFACE_2 })),
                text_color: color,
                border: Border {
                    color: if selected { ACCENT } else { BORDER_C },
                    width: 1.0,
                    radius: Radius::new(RADIUS_SM),
                },
                shadow: Shadow::default(),
            })
            .on_press(Message::KindChanged(kind))
    };

    row![
        opt(ScheduleKind::Manual, "Manual", ICON_CIRCLE),
        opt(ScheduleKind::Cron, "Cron", ICON_CLOCK),
        opt(ScheduleKind::Once, "Once", ICON_CALENDAR),
    ]
    .spacing(6)
    .into()
}

fn schedule_body<'a>(edit: &'a EditState) -> Element<'a, Message> {
    match edit.f_kind {
        ScheduleKind::Manual => text(
            "Manual tasks only run when you click Run now or trigger them externally.",
        )
        .size(12)
        .style(|_| text::Style { color: Some(TEXT_MUTED) })
        .into(),
        ScheduleKind::Once => once_picker(edit),
        ScheduleKind::Cron => cron_picker(edit),
    }
}

fn once_picker(edit: &EditState) -> Element<Message> {
    use chrono::{NaiveDate, NaiveTime};
    let date_ok = NaiveDate::parse_from_str(edit.once_date.trim(), "%Y-%m-%d").is_ok();
    let time_ok = NaiveTime::parse_from_str(edit.once_time.trim(), "%H:%M").is_ok();

    let date_field = labelled(
        "Date (UTC)",
        text_input("YYYY-MM-DD", &edit.once_date)
            .on_input(Message::OnceDateChanged)
            .padding(8)
            .width(Length::Fixed(160.0))
            .style(if date_ok { text_input_style } else { text_input_style_err }),
    );
    let time_field = labelled(
        "Time",
        text_input("HH:MM", &edit.once_time)
            .on_input(Message::OnceTimeChanged)
            .padding(8)
            .width(Length::Fixed(110.0))
            .style(if time_ok { text_input_style } else { text_input_style_err }),
    );

    column![
        row![date_field, Space::with_width(12), time_field]
            .align_y(Vertical::Center),
        Space::with_height(8),
        preview_line(&assemble_once_preview(edit, date_ok && time_ok)),
    ]
    .into()
}

fn cron_picker(edit: &EditState) -> Element<Message> {
    use chrono::NaiveTime;

    let preset_picker = pick_list(
        vec![
            CronPreset::Hourly,
            CronPreset::Daily,
            CronPreset::Weekly,
            CronPreset::Monthly,
            CronPreset::Custom,
        ],
        Some(edit.cron_preset),
        Message::CronPresetChanged,
    )
    .width(Length::Fixed(140.0))
    .padding(8)
    .style(pick_list_style);

    let time_ok = NaiveTime::parse_from_str(edit.cron_time.trim(), "%H:%M").is_ok();
    let time_field = labelled(
        "Time (UTC)",
        text_input("HH:MM", &edit.cron_time)
            .on_input(Message::CronTimeChanged)
            .padding(8)
            .width(Length::Fixed(110.0))
            .style(if time_ok { text_input_style } else { text_input_style_err }),
    );

    let minute_only_field = labelled(
        "Minute",
        text_input("0", &edit.cron_time_minute_only())
            .on_input(|s| {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit()).take(2).collect();
                Message::CronTimeChanged(format!("00:{:0>2}", cleaned))
            })
            .padding(8)
            .width(Length::Fixed(80.0))
            .style(text_input_style),
    );

    let dow_field = labelled(
        "Day of week",
        pick_list(
            DOW_OPTIONS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Some(edit.cron_dow.clone()),
            Message::CronDowChanged,
        )
        .width(Length::Fixed(120.0))
        .padding(8)
        .style(pick_list_style),
    );

    let day_field = labelled(
        "Day of month",
        text_input("1", &edit.cron_day)
            .on_input(Message::CronDayChanged)
            .padding(8)
            .width(Length::Fixed(80.0))
            .style(text_input_style),
    );

    // Sub-row that pairs with the Frequency picker on a single line. For
    // Custom we instead push a full-width block below.
    let inline_sub: Option<Element<Message>> = match edit.cron_preset {
        CronPreset::Hourly => Some(minute_only_field),
        CronPreset::Daily => Some(time_field),
        CronPreset::Weekly => Some(
            row![dow_field, Space::with_width(12), time_field]
                .align_y(Vertical::Bottom)
                .into(),
        ),
        CronPreset::Monthly => Some(
            row![day_field, Space::with_width(12), time_field]
                .align_y(Vertical::Bottom)
                .into(),
        ),
        CronPreset::Custom => None,
    };

    let top_row: Element<Message> = match inline_sub {
        Some(sub) => row![
            labelled("Frequency", preset_picker),
            Space::with_width(16),
            sub,
        ]
        .align_y(Vertical::Bottom)
        .into(),
        None => labelled("Frequency", preset_picker).into(),
    };

    let custom_block: Element<Message> = if edit.cron_preset == CronPreset::Custom {
        column![
            Space::with_height(12),
            labelled(
                "Cron expression",
                text_input("0 0 9 * * *", &edit.cron_raw)
                    .on_input(Message::CronRawChanged)
                    .padding(8)
                    .style(text_input_style),
            ),
            Space::with_height(4),
            text("6-field Quartz cron: sec min hour day mon dow.")
                .size(12)
                .style(|_| text::Style { color: Some(TEXT_FAINT) }),
        ]
        .into()
    } else {
        Space::with_height(0).into()
    };

    column![
        top_row,
        custom_block,
        Space::with_height(8),
        preview_line(&assemble_cron_preview(edit)),
    ]
    .into()
}

fn labelled<'a>(
    label: &'static str,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        text(label).size(11).style(|_| text::Style { color: Some(TEXT_MUTED) }),
        Space::with_height(4),
        body.into(),
    ]
    .into()
}

fn preview_line(expr: &str) -> Element<'static, Message> {
    let s = expr.to_string();
    row![
        text("→")
            .size(12)
            .style(|_| text::Style { color: Some(TEXT_FAINT) }),
        Space::with_width(6),
        text(s)
            .size(12)
            .style(|_| text::Style { color: Some(TEXT_MUTED) })
            .font(Font {
                family: iced::font::Family::Monospace,
                ..Font::DEFAULT
            }),
    ]
    .align_y(Vertical::Center)
    .into()
}

fn assemble_once_preview(edit: &EditState, ok: bool) -> String {
    use chrono::{NaiveDate, NaiveTime, TimeZone};
    if !ok {
        return "(invalid date or time)".into();
    }
    let d = NaiveDate::parse_from_str(edit.once_date.trim(), "%Y-%m-%d").ok();
    let t = NaiveTime::parse_from_str(edit.once_time.trim(), "%H:%M").ok();
    match (d, t) {
        (Some(d), Some(t)) => chrono::Utc
            .from_utc_datetime(&d.and_time(t))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        _ => "(invalid)".into(),
    }
}

fn assemble_cron_preview(edit: &EditState) -> String {
    use chrono::NaiveTime;
    let (h, m) = NaiveTime::parse_from_str(edit.cron_time.trim(), "%H:%M")
        .map(|t| (t.hour() as u8, t.minute() as u8))
        .unwrap_or((9, 0));
    match edit.cron_preset {
        CronPreset::Hourly => format!("0 {m} * * * *"),
        CronPreset::Daily => format!("0 {m} {h} * * *"),
        CronPreset::Weekly => format!("0 {m} {h} ? * {}", edit.cron_dow),
        CronPreset::Monthly => {
            let d: u8 = edit.cron_day.trim().parse().unwrap_or(1).clamp(1, 31);
            format!("0 {m} {h} {d} * *")
        }
        CronPreset::Custom => edit.cron_raw.trim().to_string(),
    }
}

fn perms_warning(on: bool) -> Element<'static, Message> {
    if !on {
        return Space::with_height(0).into();
    }
    row![
        Space::with_width(4),
        icon_svg(ICON_ALERT, 13.0, WARN),
        Space::with_width(6),
        text("The agent will not pause for confirmation.").size(12).style(|_| text::Style { color: Some(WARN) }),
    ]
    .align_y(Vertical::Center)
    .into()
}

fn inline_alert<'a>(kind: StatusKind, msg: &str) -> Element<'a, Message> {
    let c = kind.color();
    container(
        row![
            icon_svg(kind.icon_bytes(), 13.0, c),
            Space::with_width(8),
            text(msg.to_string()).size(13).style(|_| text::Style { color: Some(TEXT_C) }),
        ]
        .align_y(Vertical::Center),
    )
    .padding(Padding { left: 10.0, right: 10.0, top: 6.0, bottom: 6.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(SURFACE_2)),
        border: Border { color: rgba(c, 0.4), width: 1.0, radius: Radius::new(RADIUS_SM) },
        ..Default::default()
    })
    .into()
}

fn confirm_delete_bar<'a>() -> Element<'a, Message> {
    container(
        row![
            icon_svg(ICON_ALERT, 14.0, ERROR),
            Space::with_width(8),
            text("Delete this task?")
                .size(13)
                .style(|_| text::Style { color: Some(TEXT_C) })
                .font(bold()),
            Space::with_width(8),
            text("This removes it from tasks.toml.")
                .size(12)
                .style(|_| text::Style { color: Some(TEXT_MUTED) }),
            Space::with_width(Length::Fill),
            danger_icon_button(ICON_TRASH, "Confirm delete").on_press(Message::ConfirmDelete),
            Space::with_width(6),
            ghost_button("Cancel").on_press(Message::CancelDelete),
        ]
        .align_y(Vertical::Center),
    )
    .padding(Padding { left: 12.0, right: 10.0, top: 8.0, bottom: 8.0 })
    .style(|_| container::Style {
        background: Some(Background::Color(SURFACE_2)),
        border: Border { color: ERROR, width: 1.0, radius: Radius::new(RADIUS_SM) },
        ..Default::default()
    })
    .into()
}

fn primary_icon_button<'a>(
    icon_bytes: &'static [u8],
    label: &'a str,
) -> button::Button<'a, Message> {
    let content = row![
        icon_svg(icon_bytes, 14.0, Color::WHITE),
        Space::with_width(6),
        text(label).size(13).style(|_| text::Style { color: Some(Color::WHITE) }),
    ]
    .align_y(Vertical::Center);
    button(content)
        .padding(Padding { left: 10.0, right: 12.0, top: 6.0, bottom: 6.0 })
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => Color { a: 0.92, ..ACCENT },
                _ => ACCENT,
            })),
            text_color: Color::WHITE,
            border: Border { color: ACCENT, width: 1.0, radius: Radius::new(RADIUS_SM) },
            shadow: Shadow::default(),
        })
}

fn icon_button<'a>(
    icon_bytes: &'static [u8],
    label: &'a str,
    color: Color,
) -> button::Button<'a, Message> {
    let content = row![
        icon_svg(icon_bytes, 14.0, color),
        Space::with_width(6),
        text(label).size(13).style(move |_| text::Style { color: Some(color) }),
    ]
    .align_y(Vertical::Center);
    button(content)
        .padding(Padding { left: 10.0, right: 12.0, top: 6.0, bottom: 6.0 })
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => SURFACE,
                _ => SURFACE_2,
            })),
            text_color: TEXT_C,
            border: Border { color: BORDER_C, width: 1.0, radius: Radius::new(RADIUS_SM) },
            shadow: Shadow::default(),
        })
}

fn danger_icon_button<'a>(
    icon_bytes: &'static [u8],
    label: &'a str,
) -> button::Button<'a, Message> {
    let content = row![
        icon_svg(icon_bytes, 14.0, ERROR),
        Space::with_width(6),
        text(label).size(13).style(|_| text::Style { color: Some(ERROR) }),
    ]
    .align_y(Vertical::Center);
    button(content)
        .padding(Padding { left: 10.0, right: 12.0, top: 6.0, bottom: 6.0 })
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => rgba(ERROR, 0.08),
                _ => Color::TRANSPARENT,
            })),
            text_color: ERROR,
            border: Border { color: rgba(ERROR, 0.4), width: 1.0, radius: Radius::new(RADIUS_SM) },
            shadow: Shadow::default(),
        })
}

fn ghost_button<'a>(label: &'a str) -> button::Button<'a, Message> {
    button(text(label).size(13).style(|_| text::Style { color: Some(TEXT_MUTED) }))
        .padding(Padding { left: 10.0, right: 12.0, top: 6.0, bottom: 6.0 })
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => SURFACE,
                _ => Color::TRANSPARENT,
            })),
            text_color: TEXT_MUTED,
            border: Border { color: BORDER_C, width: 1.0, radius: Radius::new(RADIUS_SM) },
            shadow: Shadow::default(),
        })
}

fn text_input_style(_t: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused => ACCENT,
        _ => BORDER_C,
    };
    text_input::Style {
        background: Background::Color(SURFACE_2),
        border: Border { color: border_color, width: 1.0, radius: Radius::new(RADIUS_SM) },
        icon: TEXT_MUTED,
        placeholder: TEXT_FAINT,
        value: TEXT_C,
        selection: rgba(ACCENT, 0.4),
    }
}

/// Same as `text_input_style` but with a red border — used when the field's
/// content fails to parse so the user gets immediate feedback.
fn text_input_style_err(_t: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused => ERROR,
        _ => rgba(ERROR, 0.6),
    };
    text_input::Style {
        background: Background::Color(SURFACE_2),
        border: Border { color: border_color, width: 1.0, radius: Radius::new(RADIUS_SM) },
        icon: TEXT_MUTED,
        placeholder: TEXT_FAINT,
        value: TEXT_C,
        selection: rgba(ACCENT, 0.4),
    }
}

fn pick_list_style(_t: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border_color = match status {
        pick_list::Status::Opened => ACCENT,
        _ => BORDER_C,
    };
    pick_list::Style {
        background: Background::Color(SURFACE_2),
        border: Border { color: border_color, width: 1.0, radius: Radius::new(RADIUS_SM) },
        handle_color: TEXT_MUTED,
        placeholder_color: TEXT_FAINT,
        text_color: TEXT_C,
    }
}

fn checkbox_style(_t: &Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = matches!(
        status,
        checkbox::Status::Hovered { is_checked: true } | checkbox::Status::Active { is_checked: true }
    );
    checkbox::Style {
        background: Background::Color(if checked { ACCENT } else { SURFACE_2 }),
        icon_color: Color::WHITE,
        border: Border {
            color: if checked { ACCENT } else { BORDER_C },
            width: 1.0,
            radius: Radius::new(RADIUS_SM),
        },
        text_color: Some(TEXT_C),
    }
}

// =============================================================================
//                                MAIN
// =============================================================================

fn subscription(state: &App) -> Subscription<Message> {
    use iced::time::{self, Duration};
    let mut subs = vec![
        time::every(Duration::from_secs(1)).map(|_| Message::StatusTick),
        keyboard::on_key_press(|key, mods| match (key, mods) {
            (keyboard::Key::Character(c), m) if m.command() && c == "s" => {
                Some(Message::SaveClicked)
            }
            _ => None,
        }),
    ];
    // While viewing History, poll every 2s so scheduler-triggered runs show up
    // without needing the user to mash Refresh.
    if state.sub_tab == SubTab::History {
        subs.push(time::every(Duration::from_secs(2)).map(|_| Message::RefreshHistory));
    }
    Subscription::batch(subs)
}

pub fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let boot = boot().expect("bootstrap");
    let arc = Arc::new(boot);

    iced::application(
        |_: &App| "Opencode Orchestrator".to_string(),
        App::update,
        App::view,
    )
    .theme(|_| Theme::Dark)
    .subscription(subscription)
    .window(window::Settings {
        size: (1100.0, 720.0).into(),
        min_size: Some((720.0, 480.0).into()),
        ..Default::default()
    })
    .run_with(move || {
        let b = Arc::try_unwrap(arc.clone()).unwrap_or_else(|a| (*a).clone_for_iced());
        App::new(b)
    })
}

impl Boot {
    fn clone_for_iced(&self) -> Boot {
        Boot {
            rt: self.rt.clone(),
            cli: self.cli.clone(),
            db: self.db.clone(),
            tasks: self.tasks.clone(),
            models: self.models.clone(),
        }
    }
}
