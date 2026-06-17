// App-wide i18n. The English dictionary is the source of truth: its keys define
// `MessageKey`, and every other locale is typed as `Record<MessageKey, string>`
// so a missing or stray translation is a compile error, not a runtime blank.
//
// Strings use `{name}` placeholders, filled by t()'s params arg. Technical
// literals that must not be translated (flag names, file names, code) are left
// inline in the components, not routed through here.

export type Lang = "en" | "zh-TW";

export const LANGS: { id: Lang; label: string }[] = [
  { id: "en", label: "English" },
  { id: "zh-TW", label: "繁體中文" },
];

const STORAGE_KEY = "orchestrator.lang";

export function getInitialLang(): Lang {
  if (typeof window === "undefined") return "en";
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "zh-TW") return saved;
  } catch {
    /* localStorage may be blocked; ignore */
  }
  // First run: honour the OS/browser preference if it's Chinese (Traditional),
  // otherwise default to English.
  try {
    const nav = navigator.language?.toLowerCase() ?? "";
    if (nav.startsWith("zh") && !nav.includes("cn") && !nav.includes("hans")) {
      return "zh-TW";
    }
  } catch {
    /* navigator may be unavailable */
  }
  return "en";
}

export function saveLang(lang: Lang) {
  try {
    window.localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    /* ignore */
  }
}

/** Map an app Lang to the locale id cronstrue expects. */
export function cronLocale(lang: Lang): string {
  return lang === "zh-TW" ? "zh_TW" : "en";
}

const en = {
  // ---- App shell ----
  "app.loading": "Loading…",
  "app.tab.edit": "Edit",
  "app.tab.history": "History",
  "app.tab.historyDisabled": "Save the task first to see history",
  "app.untitled": "Untitled",
  "app.idLabel": "id: {id}",
  "app.empty.title": "No task selected",
  "app.empty.body": "Pick a task on the left or create a new one.",
  "app.status.ready": "ready",
  "app.status.tasksOne": "{count} task · {enabled} enabled",
  "app.status.tasksMany": "{count} tasks · {enabled} enabled",
  "app.flash.runStarted": "Run #{id} started",
  "app.flash.runFinished": "Run #{id} {status}",
  "app.flash.loadFailed": "Load failed: {error}",
  "app.flash.savedRestarted": "Saved & scheduler restarted",
  "app.flash.deleted": "Deleted",
  "app.flash.reordered": "Reordered",
  "app.flash.duplicated": "Duplicated — review and save to create the copy",

  // ---- Update banner ----
  "update.available": "Version {version} is available.",
  "update.install": "Update now",
  "update.downloading": "Downloading update… {percent}%",
  "update.ready": "Update installed — restarting…",
  "update.error": "Update failed: {error}",
  "update.retry": "Retry",
  "update.dismiss": "Dismiss",

  // ---- Sidebar ----
  "sidebar.tasks": "Tasks · {count}",
  "sidebar.newTask": "New task",
  "sidebar.search": "Search…",
  "sidebar.noTasks": "No tasks yet.",
  "sidebar.createOne": "Create one",
  "sidebar.noMatch": "No tasks match.",
  "sidebar.unnamed": "(unnamed)",
  "sidebar.running": "running",
  "sidebar.unsaved": "Unsaved changes",
  "sidebar.draft": "draft",
  "sidebar.draftTitle": "Unsaved new task — save it to keep it",
  "sidebar.disabled": "disabled",
  "sidebar.settings": "Settings",

  // ---- Settings panel ----
  "settings.title": "Settings",
  "settings.binary.section": "opencode binary",
  "settings.binary.label": "Absolute path to the `opencode` binary",
  "settings.binary.placeholder": "(leave empty to fall back to PATH lookup)",
  "settings.binary.help":
    "Production setups should set this explicitly — PATH lookup is vulnerable to PATH hijacking.",
  "settings.binary.resolved": "resolved: {path}",
  "settings.language.section": "Language",
  "settings.language.label": "Interface language",
  "settings.language.help": "Applies immediately and is remembered on this machine.",
  "settings.history.section": "Run history",
  "settings.history.label": "Max runs to keep per task",
  "settings.history.placeholder": "unlimited",
  "settings.history.help":
    "After each run, older finished runs beyond this count (and their logs, events, and comments) are deleted. Leave empty or 0 to keep every run.",
  "settings.nav": "On this page",
  "settings.storage.section": "Storage",
  "settings.storage.loading": "Loading paths…",
  "settings.storage.config.label": "Task config",
  "settings.storage.config.note": "tasks.toml — task definitions and settings.",
  "settings.storage.runsdb.label": "Run history db",
  "settings.storage.runsdb.note":
    "runs.db — this app's SQLite store for runs, events, and logs.",
  "settings.storage.sessiondb.label": "opencode session db",
  "settings.storage.sessiondb.note":
    "opencode.db — owned by the opencode CLI; read-only here, used to render conversations.",
  "settings.storage.worktree.label": "Worktree root",
  "settings.storage.worktree.note":
    "OS temp dir. Worktree-enabled runs create opencode-orchestrator-wt-<uuid>/ folders here and remove them when the run ends.",
  "settings.saved": "Saved.",
  "settings.saveFailed": "Save failed: {error}",

  // ---- Common buttons ----
  "btn.browse": "Browse…",
  "btn.revert": "Revert",
  "btn.save": "Save",
  "btn.saving": "Saving…",
  "btn.cancel": "Cancel",

  // ---- Theme toggle ----
  "theme.switchToLight": "Switch to light mode",
  "theme.switchToDark": "Switch to dark mode",

  // ---- Edit tab ----
  "edit.runNow": "Run now",
  "edit.duplicate": "Duplicate",
  "edit.nav": "On this page",
  "edit.nameCopySuffix": "{name} (copy)",
  "edit.confirmDeleteQ": "Delete this task?",
  "edit.confirmDelete": "Confirm delete",
  "edit.delete": "Delete",
  "edit.discard": "Discard",
  "edit.savedRestarted": "Saved. Scheduler restarted.",
  "edit.section.basics": "Basics",
  "edit.name": "Name",
  "edit.tags": "Tags",
  "edit.tagsPlaceholder": "review, daily, frontend…",
  "edit.removeTag": "Remove tag {tag}",
  "edit.section.schedule": "Schedule",
  "edit.enabled": "Enabled",
  "edit.section.execution": "Execution",
  "edit.workingDir": "Working directory",
  "edit.model": "Model",
  "edit.modelDefault": "(opencode default)",
  "edit.timeout": "Timeout (minutes)",
  "edit.timeout.set": "gracefully cancel runs that exceed {secs}s",
  "edit.timeout.none": "no timeout — run can take as long as opencode needs",
  "edit.skipPerms.warn":
    "opencode will run without prompting you to allow tool calls.",
  "edit.worktree.toggle": "Run in throwaway git worktree",
  "edit.worktree.help":
    "The fresh worktree starts clean — any gitignored files (e.g. .env, node_modules, build output) won't be there. To copy some in, drop a .worktreeinclude file at the repo root listing one path per line (# for comments). Paths tracked by git are refused.",
  "edit.worktree.baseLabel": "Worktree base ref (optional)",
  "edit.worktree.basePlaceholder": "leave empty to fork from current HEAD",
  "edit.worktree.baseHelp":
    "When set (e.g. origin/main), the runner does git fetch --all first, verifies the ref, then creates the worktree from it; any failure aborts the run with no HEAD fallback.",
  "edit.section.prompt": "Prompt",
  "edit.promptPlaceholder": "Describe the work for opencode to do…",
  "edit.promptStats": "{chars} chars · {lines} lines",
  "edit.validate.name": "Name is required.",
  "edit.validate.workingDir": "Working directory is required.",
  "edit.validate.prompt": "Prompt is empty.",
  "edit.validate.cron": "Cron expression is empty.",
  "edit.validate.once": "Once timestamp is empty.",

  // ---- Memory ----
  "edit.section.memory": "Memory",
  "edit.memory.enable": "Enable memory & feedback",
  "edit.memory.enableHelp":
    "Inject this task's saved memory and recent comments into the prompt, and give the agent memory tools (over MCP) it can call mid-run to update this memory.",
  "edit.memory.label": "Saved memory",
  "edit.memory.placeholder":
    "Empty. The agent fills this in by calling its memory tools during a run — or you can edit it here directly.",
  "edit.memory.save": "Save memory",
  "edit.memory.clear": "Clear",
  "edit.memory.saved": "Memory saved.",
  "edit.memory.updated": "Updated {time}",
  "edit.memory.loadFailed": "Failed to load memory: {error}",
  "edit.memory.saveFailed": "Failed to save memory: {error}",

  // ---- Schedule editor ----
  "sched.kind.manual": "Manual",
  "sched.kind.cron": "Cron",
  "sched.kind.once": "Once",
  "sched.manualHelp": "Manual tasks only run when you click Run now.",
  "sched.tzHelp": "Schedules run in your machine's timezone: {tz} ({offset}).",
  "sched.local": "local",
  "sched.once.date": "Date",
  "sched.once.time": "Time",
  "sched.once.willFire": "Will fire on {when}.",
  "sched.once.storedAs": "Stored as {value}.",
  "sched.preset.hourly": "Hourly",
  "sched.preset.daily": "Daily",
  "sched.preset.weekly": "Weekly",
  "sched.preset.monthly": "Monthly",
  "sched.preset.custom": "Custom",
  "sched.atMinute": "At minute (0–59)",
  "sched.time": "Time",
  "sched.onDays": "On days",
  "sched.onDay": "On day (1–31)",
  "sched.custom.label":
    "Quartz cron expression (6–7 fields: sec min hour day month dow [year])",
  "sched.custom.plain": "In plain language: {desc}",
  "sched.custom.help":
    "Quartz can't accept specific values in both day-of-month and day-of-week at the same time — use ? in the field you aren't constraining.",
  "sched.nextFire": "Next fire: {when}.",
  "sched.expression": "Expression: {expr}",

  // ---- Weekday short names (schedule editor day buttons) ----
  "wd.mon": "Mon",
  "wd.tue": "Tue",
  "wd.wed": "Wed",
  "wd.thu": "Thu",
  "wd.fri": "Fri",
  "wd.sat": "Sat",
  "wd.sun": "Sun",

  // ---- Cron describe (compact chip labels) ----
  "crondesc.hourly": "Hourly · :{min}",
  "crondesc.daily": "Daily · {time}",
  "crondesc.weekly": "Weekly · {days} {time}",
  "crondesc.monthly": "Monthly · day {day} · {time}",
  "crondesc.everyDay": "every day",
  "crondesc.once": "Once · {when}",
  "crondesc.manual": "Manual",
  "crondesc.listSep": ", ",
  "crondesc.rangeSep": "–",

  // ---- History tab ----
  "hist.runs": "Runs · {count}",
  "hist.clearing": "Clearing…",
  "hist.confirmClear": "Confirm clear",
  "hist.clearAria": "Clear history",
  "hist.clearTitle": "Clear finished runs for this task",
  "hist.refreshAria": "Refresh runs",
  "hist.refreshTitle": "Refresh",
  "hist.noRuns": "No runs yet for this task.",
  "hist.stalled": "stalled",
  "hist.stalledChipTitle":
    "Killed after a long silence — likely a stalled model stream or hung tool call, not real work",
  "hist.startedMeta": "started {time} · {duration}",
  "hist.selectRun": "Select a run on the left.",
  "hist.runSeq": "Run #{seq}",
  "hist.dbIdTitle": "Internal db id",
  "hist.dbId": "db #{id}",
  "hist.stop": "Stop",
  "hist.started": "started {time}",
  "hist.finished": " · finished {time}",
  "hist.session": " · session {id}",
  "hist.error": "Error",
  "hist.stalledSection": "Stalled",
  "hist.stalledBody":
    "Active for {active} of {wall} — no output for {silent} before it was stopped. opencode was blocked waiting (a stalled model stream or a hung tool call), not doing work.",
  "hist.steps": "Steps",
  "hist.waitingFirstStep": "Waiting for first step…",
  "hist.noSteps": "No steps recorded.",
  "hist.collapse": "Collapse",
  "hist.expand": "Expand",
  "hist.output": "Output",
  "hist.logs.waiting": "waiting…",
  "hist.logs.none": "no output captured",
  "hist.logs.linesOne": "{count} line",
  "hist.logs.linesMany": "{count} lines",
  "hist.logs.stderr": " · {count} stderr",
  "hist.logs.jumpToLive": "jump to live",
  "hist.logs.scrollBottom": "Scroll to bottom",
  "hist.logs.noneYet": "No output captured yet.",
  "hist.conversation": "Conversation",
  "hist.convo.waitingSession": "Waiting for opencode to allocate a session…",
  "hist.convo.noSession": "No session id captured for this run.",
  "hist.convo.loading": "Loading…",
  "hist.convo.streaming": "Streaming — first message will appear shortly…",
  "hist.convo.empty": "Conversation is empty.",
  "hist.reasoning": "Reasoning",
  "hist.tool.input": "input",
  "hist.tool.output": "output",
  "hist.tab.log": "Run log",
  "hist.tab.comments": "Comments",
  "hist.prompt": "Sent prompt",
  "hist.prompt.summary": "{chars} chars — exactly what was sent to opencode",
  "hist.comments": "Comments",
  "hist.comments.hint":
    "Comments on this task's runs are fed into the next run as feedback (most recent first).",
  "hist.comments.empty": "No comments on this run yet.",
  "hist.comments.placeholder": "Add feedback for the next run…",
  "hist.comments.add": "Add comment",
  "hist.comments.adding": "Adding…",
  "hist.comments.delete": "Delete comment",
} as const;

export type MessageKey = keyof typeof en;

const zhTW: Record<MessageKey, string> = {
  // ---- App shell ----
  "app.loading": "載入中…",
  "app.tab.edit": "編輯",
  "app.tab.history": "歷史紀錄",
  "app.tab.historyDisabled": "請先儲存任務才能查看歷史紀錄",
  "app.untitled": "未命名",
  "app.idLabel": "id：{id}",
  "app.empty.title": "尚未選取任務",
  "app.empty.body": "從左側挑一個任務，或建立新的任務。",
  "app.status.ready": "就緒",
  "app.status.tasksOne": "{count} 個任務 · {enabled} 個已啟用",
  "app.status.tasksMany": "{count} 個任務 · {enabled} 個已啟用",
  "app.flash.runStarted": "執行 #{id} 已開始",
  "app.flash.runFinished": "執行 #{id} {status}",
  "app.flash.loadFailed": "載入失敗：{error}",
  "app.flash.savedRestarted": "已儲存，排程器已重啟",
  "app.flash.deleted": "已刪除",
  "app.flash.reordered": "已重新排序",
  "app.flash.duplicated": "已複製——請檢視後儲存以建立副本",

  // ---- Update banner ----
  "update.available": "有新版本 {version} 可用。",
  "update.install": "立即更新",
  "update.downloading": "下載更新中… {percent}%",
  "update.ready": "更新完成，正在重新啟動…",
  "update.error": "更新失敗：{error}",
  "update.retry": "重試",
  "update.dismiss": "關閉",

  // ---- Sidebar ----
  "sidebar.tasks": "任務 · {count}",
  "sidebar.newTask": "新增任務",
  "sidebar.search": "搜尋…",
  "sidebar.noTasks": "尚無任務。",
  "sidebar.createOne": "建立一個",
  "sidebar.noMatch": "沒有符合的任務。",
  "sidebar.unnamed": "（未命名）",
  "sidebar.running": "執行中",
  "sidebar.unsaved": "有未儲存的修改",
  "sidebar.draft": "草稿",
  "sidebar.draftTitle": "尚未儲存的新任務——儲存後才會保留",
  "sidebar.disabled": "已停用",
  "sidebar.settings": "設定",

  // ---- Settings panel ----
  "settings.title": "設定",
  "settings.binary.section": "opencode 執行檔",
  "settings.binary.label": "`opencode` 執行檔的絕對路徑",
  "settings.binary.placeholder": "（留空則改用 PATH 搜尋）",
  "settings.binary.help":
    "正式環境應明確指定此路徑——PATH 搜尋容易遭受 PATH 劫持攻擊。",
  "settings.binary.resolved": "已解析：{path}",
  "settings.language.section": "語言",
  "settings.language.label": "介面語言",
  "settings.language.help": "立即套用，並記住於此電腦。",
  "settings.history.section": "執行歷史保留",
  "settings.history.label": "每個任務最多保留的執行數",
  "settings.history.placeholder": "不限制",
  "settings.history.help":
    "每次執行後，超過此數量的較舊已結束執行紀錄（連同其日誌、事件與留言）會被刪除。留空或填 0 表示保留所有執行。",
  "settings.nav": "本頁目錄",
  "settings.storage.section": "儲存位置",
  "settings.storage.loading": "載入路徑中…",
  "settings.storage.config.label": "任務設定檔",
  "settings.storage.config.note": "tasks.toml — 任務定義與設定。",
  "settings.storage.runsdb.label": "執行歷史資料庫",
  "settings.storage.runsdb.note":
    "runs.db — 本程式用來儲存執行、事件與日誌的 SQLite 資料庫。",
  "settings.storage.sessiondb.label": "opencode 工作階段資料庫",
  "settings.storage.sessiondb.note":
    "opencode.db — 由 opencode CLI 擁有；此處唯讀，用來呈現對話內容。",
  "settings.storage.worktree.label": "Worktree 根目錄",
  "settings.storage.worktree.note":
    "作業系統暫存目錄。啟用 worktree 的執行會在此建立 opencode-orchestrator-wt-<uuid>/ 資料夾，並在執行結束時移除。",
  "settings.saved": "已儲存。",
  "settings.saveFailed": "儲存失敗：{error}",

  // ---- Common buttons ----
  "btn.browse": "瀏覽…",
  "btn.revert": "還原",
  "btn.save": "儲存",
  "btn.saving": "儲存中…",
  "btn.cancel": "取消",

  // ---- Theme toggle ----
  "theme.switchToLight": "切換為淺色模式",
  "theme.switchToDark": "切換為深色模式",

  // ---- Edit tab ----
  "edit.runNow": "立即執行",
  "edit.duplicate": "複製任務",
  "edit.nav": "本頁目錄",
  "edit.nameCopySuffix": "{name}（副本）",
  "edit.confirmDeleteQ": "確定刪除此任務？",
  "edit.confirmDelete": "確認刪除",
  "edit.delete": "刪除",
  "edit.discard": "捨棄",
  "edit.savedRestarted": "已儲存，排程器已重啟。",
  "edit.section.basics": "基本資料",
  "edit.name": "名稱",
  "edit.tags": "標籤",
  "edit.tagsPlaceholder": "review、daily、frontend…",
  "edit.removeTag": "移除標籤 {tag}",
  "edit.section.schedule": "排程",
  "edit.enabled": "已啟用",
  "edit.section.execution": "執行設定",
  "edit.workingDir": "工作目錄",
  "edit.model": "模型",
  "edit.modelDefault": "（opencode 預設）",
  "edit.timeout": "逾時（分鐘）",
  "edit.timeout.set": "對超過 {secs} 秒的執行進行優雅取消",
  "edit.timeout.none": "不逾時——執行可依 opencode 所需時間進行",
  "edit.skipPerms.warn": "opencode 將直接執行，不會提示你允許工具呼叫。",
  "edit.worktree.toggle": "在拋棄式 git worktree 中執行",
  "edit.worktree.help":
    "全新的 worktree 會從乾淨狀態開始——任何被 gitignore 的檔案（例如 .env、node_modules、建置輸出）都不會出現。若要帶入部分檔案，在 repo 根目錄放一個 .worktreeinclude 檔案，每行列出一個路徑（# 為註解）。已被 git 追蹤的路徑會被拒絕。",
  "edit.worktree.baseLabel": "Worktree 基準 ref（選填）",
  "edit.worktree.basePlaceholder": "留空則從目前 HEAD 分出",
  "edit.worktree.baseHelp":
    "設定後（例如 origin/main），執行器會先做 git fetch --all、驗證該 ref，再從它建立 worktree；任何失敗都會中止執行，不會退回 HEAD。",
  "edit.section.prompt": "提示詞",
  "edit.promptPlaceholder": "描述要交給 opencode 執行的工作…",
  "edit.promptStats": "{chars} 字元 · {lines} 行",
  "edit.validate.name": "名稱為必填。",
  "edit.validate.workingDir": "工作目錄為必填。",
  "edit.validate.prompt": "提示詞不可為空。",
  "edit.validate.cron": "Cron 表達式不可為空。",
  "edit.validate.once": "單次時間戳記不可為空。",

  // ---- Memory ----
  "edit.section.memory": "記憶",
  "edit.memory.enable": "啟用記憶與回饋",
  "edit.memory.enableHelp":
    "把這個任務已儲存的記憶與最近的留言注入提示詞，並提供 agent 一組記憶工具（透過 MCP），讓它在執行中自行更新這份記憶。",
  "edit.memory.label": "已儲存的記憶",
  "edit.memory.placeholder":
    "目前為空。agent 會在執行中呼叫記憶工具來寫入這裡——你也可以直接在此編輯。",
  "edit.memory.save": "儲存記憶",
  "edit.memory.clear": "清空",
  "edit.memory.saved": "記憶已儲存。",
  "edit.memory.updated": "更新於 {time}",
  "edit.memory.loadFailed": "載入記憶失敗：{error}",
  "edit.memory.saveFailed": "儲存記憶失敗：{error}",

  // ---- Schedule editor ----
  "sched.kind.manual": "手動",
  "sched.kind.cron": "Cron",
  "sched.kind.once": "單次",
  "sched.manualHelp": "手動任務只有在你點選「立即執行」時才會執行。",
  "sched.tzHelp": "排程依你電腦的時區執行：{tz}（{offset}）。",
  "sched.local": "本機",
  "sched.once.date": "日期",
  "sched.once.time": "時間",
  "sched.once.willFire": "將於 {when} 觸發。",
  "sched.once.storedAs": "儲存為 {value}。",
  "sched.preset.hourly": "每小時",
  "sched.preset.daily": "每天",
  "sched.preset.weekly": "每週",
  "sched.preset.monthly": "每月",
  "sched.preset.custom": "自訂",
  "sched.atMinute": "於第幾分（0–59）",
  "sched.time": "時間",
  "sched.onDays": "於星期",
  "sched.onDay": "於每月第幾日（1–31）",
  "sched.custom.label":
    "Quartz cron 表達式（6–7 欄：秒 分 時 日 月 星期 [年]）",
  "sched.custom.plain": "白話：{desc}",
  "sched.custom.help":
    "Quartz 無法同時在「日」與「星期」兩欄指定具體值——在你不限制的那一欄使用 ?。",
  "sched.nextFire": "下次觸發：{when}。",
  "sched.expression": "表達式：{expr}",

  // ---- Weekday short names ----
  "wd.mon": "週一",
  "wd.tue": "週二",
  "wd.wed": "週三",
  "wd.thu": "週四",
  "wd.fri": "週五",
  "wd.sat": "週六",
  "wd.sun": "週日",

  // ---- Cron describe (compact chip labels) ----
  "crondesc.hourly": "每小時 · :{min}",
  "crondesc.daily": "每天 · {time}",
  "crondesc.weekly": "每週 · {days} {time}",
  "crondesc.monthly": "每月 · {day} 號 · {time}",
  "crondesc.everyDay": "每天",
  "crondesc.once": "單次 · {when}",
  "crondesc.manual": "手動",
  "crondesc.listSep": "、",
  "crondesc.rangeSep": "–",

  // ---- History tab ----
  "hist.runs": "執行 · {count}",
  "hist.clearing": "清除中…",
  "hist.confirmClear": "確認清除",
  "hist.clearAria": "清除歷史紀錄",
  "hist.clearTitle": "清除此任務已結束的執行紀錄",
  "hist.refreshAria": "重新整理執行",
  "hist.refreshTitle": "重新整理",
  "hist.noRuns": "此任務尚無執行紀錄。",
  "hist.stalled": "停滯",
  "hist.stalledChipTitle":
    "長時間無輸出後被終止——可能是模型串流停滯或工具呼叫卡住，並非真正在工作",
  "hist.startedMeta": "開始於 {time} · {duration}",
  "hist.selectRun": "從左側選取一筆執行。",
  "hist.runSeq": "執行 #{seq}",
  "hist.dbIdTitle": "內部資料庫 id",
  "hist.dbId": "db #{id}",
  "hist.stop": "停止",
  "hist.started": "開始於 {time}",
  "hist.finished": " · 結束於 {time}",
  "hist.session": " · 工作階段 {id}",
  "hist.error": "錯誤",
  "hist.stalledSection": "停滯",
  "hist.stalledBody":
    "在 {wall} 中有 {active} 在活動——被停止前有 {silent} 沒有任何輸出。opencode 當時卡在等待（模型串流停滯或工具呼叫卡住），並非在工作。",
  "hist.steps": "步驟",
  "hist.waitingFirstStep": "等待第一個步驟…",
  "hist.noSteps": "沒有紀錄到步驟。",
  "hist.collapse": "收合",
  "hist.expand": "展開",
  "hist.output": "輸出",
  "hist.logs.waiting": "等待中…",
  "hist.logs.none": "未擷取到輸出",
  "hist.logs.linesOne": "{count} 行",
  "hist.logs.linesMany": "{count} 行",
  "hist.logs.stderr": " · {count} 筆 stderr",
  "hist.logs.jumpToLive": "跳至即時",
  "hist.logs.scrollBottom": "捲動至底部",
  "hist.logs.noneYet": "尚未擷取到輸出。",
  "hist.conversation": "對話",
  "hist.convo.waitingSession": "等待 opencode 配置工作階段…",
  "hist.convo.noSession": "此執行未擷取到工作階段 id。",
  "hist.convo.loading": "載入中…",
  "hist.convo.streaming": "串流中——第一則訊息即將出現…",
  "hist.convo.empty": "對話是空的。",
  "hist.reasoning": "推理",
  "hist.tool.input": "輸入",
  "hist.tool.output": "輸出",
  "hist.tab.log": "執行紀錄",
  "hist.tab.comments": "留言",
  "hist.prompt": "送出的提示詞",
  "hist.prompt.summary": "{chars} 字元——實際送給 opencode 的完整內容",
  "hist.comments": "留言",
  "hist.comments.hint": "此任務各次執行的留言會作為回饋注入下一次執行（最新的在前）。",
  "hist.comments.empty": "這次執行還沒有留言。",
  "hist.comments.placeholder": "為下一次執行新增回饋…",
  "hist.comments.add": "新增留言",
  "hist.comments.adding": "新增中…",
  "hist.comments.delete": "刪除留言",
};

const DICTS: Record<Lang, Record<MessageKey, string>> = {
  en,
  "zh-TW": zhTW,
};

export function t(
  lang: Lang,
  key: MessageKey,
  params?: Record<string, string | number>,
): string {
  let s = DICTS[lang][key] ?? en[key];
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      s = s.split(`{${k}}`).join(String(v));
    }
  }
  return s;
}
