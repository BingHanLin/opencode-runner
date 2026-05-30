// Widget-driven editor for the `schedule` string. Hides the underlying
// "manual" / "cron:<expr>" / "once:<rfc3339>" serialization behind native
// date/time pickers plus a small cron preset wizard (Hourly / Daily /
// Weekly / Monthly / Custom). Round-trips the encoded value on every edit
// so the parent doesn't need to know about the widget shape.

import { useEffect, useMemo, useState } from "react";

type Kind = "manual" | "cron" | "once";

interface Props {
  schedule: string;
  onChange: (next: string) => void;
}

const KINDS: { id: Kind; label: string }[] = [
  { id: "manual", label: "Manual" },
  { id: "cron", label: "Cron" },
  { id: "once", label: "Once" },
];

export function ScheduleEditor({ schedule, onChange }: Props) {
  const kind: Kind = schedule.startsWith("cron:")
    ? "cron"
    : schedule.startsWith("once:")
      ? "once"
      : "manual";

  function setKind(next: Kind) {
    if (next === kind) return;
    if (next === "manual") return onChange("manual");
    if (next === "once") {
      // Default to "tomorrow 09:00 local" when switching in.
      const d = new Date();
      d.setDate(d.getDate() + 1);
      d.setHours(9, 0, 0, 0);
      return onChange(`once:${toRfc3339(d)}`);
    }
    // Switching to cron — default to "daily 09:00".
    return onChange("cron:0 0 9 ? * *");
  }

  return (
    <>
      <div className="row" style={{ gap: 6, marginBottom: 14 }}>
        {KINDS.map((k) => (
          <button
            key={k.id}
            type="button"
            className={`btn ${kind === k.id ? "primary" : ""}`}
            onClick={() => setKind(k.id)}
          >
            {k.label}
          </button>
        ))}
      </div>

      {kind === "manual" && (
        <div className="help">
          Manual tasks only run when you click <strong>Run now</strong>.
        </div>
      )}

      {kind === "cron" && (
        <CronEditor
          expr={schedule.slice(5)}
          onChange={(e) => onChange(`cron:${e}`)}
        />
      )}

      {kind === "once" && (
        <OnceEditor
          value={schedule.slice(5)}
          onChange={(v) => onChange(`once:${v}`)}
        />
      )}

      {kind !== "manual" && (
        <div className="help" style={{ marginTop: 12 }}>
          <span className="chip accent" style={{ marginRight: 6 }}>
            local
          </span>
          Schedules run in your machine's timezone:{" "}
          <strong>{localTz().name}</strong> ({localTz().offset}).
        </div>
      )}
    </>
  );
}

// Local timezone label, reads once per render — cheap.
function localTz(): { name: string; offset: string } {
  let name = "local";
  try {
    name = Intl.DateTimeFormat().resolvedOptions().timeZone || "local";
  } catch {
    /* very old browsers */
  }
  const minutes = -new Date().getTimezoneOffset();
  const sign = minutes >= 0 ? "+" : "-";
  const abs = Math.abs(minutes);
  return {
    name,
    offset: `UTC${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`,
  };
}

function formatLocalDateTime(d: Date): string {
  try {
    return new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
      weekday: "short",
    }).format(d);
  } catch {
    return d.toLocaleString();
  }
}

// ============================================================================
//                                  ONCE
// ============================================================================

function OnceEditor({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  const parsed = useMemo(() => parseRfc3339Local(value), [value]);

  function emit(date: string, time: string) {
    if (!date || !time) {
      // Preserve raw if either side is empty so the user can keep typing.
      onChange(value);
      return;
    }
    const dt = new Date(`${date}T${time}:00`);
    if (Number.isNaN(dt.getTime())) {
      onChange(value);
      return;
    }
    onChange(toRfc3339(dt));
  }

  return (
    <>
      <div className="row" style={{ gap: 10, alignItems: "flex-end" }}>
        <div className="field" style={{ flex: 1, marginBottom: 0 }}>
          <label className="field-label">Date</label>
          <input
            type="date"
            className="input"
            value={parsed.date}
            onChange={(e) => emit(e.target.value, parsed.time)}
          />
        </div>
        <div className="field" style={{ flex: 1, marginBottom: 0 }}>
          <label className="field-label">Time</label>
          <input
            type="time"
            className="input"
            value={parsed.time}
            onChange={(e) => emit(parsed.date, e.target.value)}
          />
        </div>
      </div>
      {parsed.date && parsed.time && (
        <div className="help" style={{ marginTop: 10 }}>
          Will fire on{" "}
          <strong>
            {formatLocalDateTime(new Date(`${parsed.date}T${parsed.time}:00`))}
          </strong>
          .
        </div>
      )}
      <div className="help" style={{ marginTop: 4 }}>
        Stored as <code>{value || "—"}</code>.
      </div>
    </>
  );
}

function parseRfc3339Local(s: string): { date: string; time: string } {
  if (!s) return { date: "", time: "" };
  // Parse as a Date so timezone offsets are converted to the user's local
  // wall-clock for the picker — feels more natural than dumping UTC.
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return { date: "", time: "" };
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    date: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
    time: `${pad(d.getHours())}:${pad(d.getMinutes())}`,
  };
}

function toRfc3339(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const tz = -d.getTimezoneOffset();
  const sign = tz >= 0 ? "+" : "-";
  const abs = Math.abs(tz);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}:00` +
    `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
  );
}

// ============================================================================
//                                  CRON
// ============================================================================

type CronPreset = "hourly" | "daily" | "weekly" | "monthly" | "custom";

const WEEKDAYS = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"] as const;
const QUARTZ_ORDER = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"] as const;
type WeekDay = (typeof WEEKDAYS)[number];

interface CronState {
  preset: CronPreset;
  minute: number;
  hour: number;
  days: WeekDay[];
  dom: number;
}

const DEFAULT_CRON: CronState = {
  preset: "daily",
  minute: 0,
  hour: 9,
  days: ["MON"],
  dom: 1,
};

function CronEditor({
  expr,
  onChange,
}: {
  expr: string;
  onChange: (next: string) => void;
}) {
  const derived = useMemo<CronState>(() => parseCronExpr(expr), [expr]);
  // Local flag so clicking "Custom" sticks. Without it, parseCronExpr would
  // keep round-tripping back to whichever preset the current expression
  // happens to match. Force-on when the expression doesn't match any preset.
  const [customMode, setCustomMode] = useState<boolean>(
    derived.preset === "custom",
  );
  useEffect(() => {
    if (derived.preset === "custom") setCustomMode(true);
  }, [derived.preset]);

  const preset: CronPreset = customMode ? "custom" : derived.preset;

  function pickPreset(p: CronPreset) {
    if (p === "custom") {
      setCustomMode(true);
      return;
    }
    setCustomMode(false);
    const merged: CronState = { ...derived, preset: p };
    onChange(buildCronExpr(merged, expr));
  }

  function update(next: Partial<CronState>) {
    const merged: CronState = { ...derived, ...next, preset };
    onChange(buildCronExpr(merged, expr));
  }

  function timeValue() {
    return `${pad(derived.hour)}:${pad(derived.minute)}`;
  }
  function setTime(v: string) {
    const m = v.match(/^(\d{1,2}):(\d{2})$/);
    if (!m) return;
    update({ hour: clamp(parseInt(m[1], 10), 0, 23), minute: clamp(parseInt(m[2], 10), 0, 59) });
  }

  return (
    <>
      <div className="row" style={{ gap: 6, marginBottom: 14, flexWrap: "wrap" }}>
        {(["hourly", "daily", "weekly", "monthly", "custom"] as CronPreset[]).map((p) => (
          <button
            key={p}
            type="button"
            className={`btn ${preset === p ? "primary" : ""}`}
            onClick={() => pickPreset(p)}
          >
            {capitalize(p)}
          </button>
        ))}
      </div>

      {preset === "hourly" && (
        <div className="field">
          <label className="field-label">At minute (0–59)</label>
          <input
            type="number"
            min={0}
            max={59}
            className="input"
            style={{ maxWidth: 120 }}
            value={derived.minute}
            onChange={(e) =>
              update({ minute: clamp(parseInt(e.target.value, 10) || 0, 0, 59) })
            }
          />
        </div>
      )}

      {preset === "daily" && (
        <div className="field">
          <label className="field-label">Time</label>
          <input
            type="time"
            className="input"
            style={{ maxWidth: 160 }}
            value={timeValue()}
            onChange={(e) => setTime(e.target.value)}
          />
        </div>
      )}

      {preset === "weekly" && (
        <>
          <div className="field">
            <label className="field-label">Time</label>
            <input
              type="time"
              className="input"
              style={{ maxWidth: 160 }}
              value={timeValue()}
              onChange={(e) => setTime(e.target.value)}
            />
          </div>
          <div className="field">
            <label className="field-label">On days</label>
            <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
              {WEEKDAYS.map((d) => {
                const on = derived.days.includes(d);
                return (
                  <button
                    key={d}
                    type="button"
                    className={`btn ${on ? "primary" : ""}`}
                    style={{ minWidth: 56, padding: "6px 10px" }}
                    onClick={() => {
                      const next = on
                        ? derived.days.filter((x) => x !== d)
                        : [...derived.days, d];
                      update({ days: next.length ? next : ["MON"] });
                    }}
                  >
                    {capitalize(d.toLowerCase())}
                  </button>
                );
              })}
            </div>
          </div>
        </>
      )}

      {preset === "monthly" && (
        <div className="row" style={{ gap: 10, alignItems: "flex-end" }}>
          <div className="field" style={{ marginBottom: 0 }}>
            <label className="field-label">On day (1–31)</label>
            <input
              type="number"
              min={1}
              max={31}
              className="input"
              style={{ maxWidth: 100 }}
              value={derived.dom}
              onChange={(e) =>
                update({ dom: clamp(parseInt(e.target.value, 10) || 1, 1, 31) })
              }
            />
          </div>
          <div className="field" style={{ marginBottom: 0 }}>
            <label className="field-label">Time</label>
            <input
              type="time"
              className="input"
              style={{ maxWidth: 160 }}
              value={timeValue()}
              onChange={(e) => setTime(e.target.value)}
            />
          </div>
        </div>
      )}

      {preset === "custom" && (
        <>
          <div className="field">
            <label className="field-label">
              Quartz cron expression (6–7 fields: sec min hour day month dow [year])
            </label>
            <input
              className="input"
              value={expr}
              placeholder="0 0 9 ? * MON-FRI"
              onChange={(e) => onChange(e.target.value)}
            />
          </div>
          <div className="help">
            Quartz can't accept specific values in both <em>day-of-month</em> and{" "}
            <em>day-of-week</em> at the same time — use <code>?</code> in the
            field you aren't constraining.
          </div>
        </>
      )}

      {preset !== "custom" && nextFire(preset, derived) && (
        <div className="help" style={{ marginTop: 10 }}>
          Next fire:{" "}
          <strong>{formatLocalDateTime(nextFire(preset, derived)!)}</strong>.
        </div>
      )}
      <div className="help" style={{ marginTop: 4 }}>
        Expression: <code>{expr || "—"}</code>
      </div>
    </>
  );
}

function parseCronExpr(expr: string): CronState {
  const parts = expr.trim().split(/\s+/);
  if (parts.length < 6) return DEFAULT_CRON;
  const [sec, min, hour, dom, _month, dow] = parts;
  if (sec !== "0") return { ...DEFAULT_CRON, preset: "custom" };
  const minN = parseIntStrict(min, 0, 59);
  if (minN == null) return { ...DEFAULT_CRON, preset: "custom" };

  // Hourly: minute fixed, hour=*, dom=?, dow=*
  if (hour === "*" && dom === "?" && dow === "*") {
    return { ...DEFAULT_CRON, preset: "hourly", minute: minN };
  }
  const hourN = parseIntStrict(hour, 0, 23);
  if (hourN == null) return { ...DEFAULT_CRON, preset: "custom" };

  // Daily: dom=?, dow=*
  if (dom === "?" && dow === "*") {
    return { ...DEFAULT_CRON, preset: "daily", minute: minN, hour: hourN };
  }
  // Weekly: dom=?, dow=<list|range>
  if (dom === "?") {
    const days = parseWeekdayList(dow);
    if (days)
      return {
        ...DEFAULT_CRON,
        preset: "weekly",
        minute: minN,
        hour: hourN,
        days,
      };
  }
  // Monthly: dom=N, dow=?
  if (dow === "?") {
    const domN = parseIntStrict(dom, 1, 31);
    if (domN != null)
      return {
        ...DEFAULT_CRON,
        preset: "monthly",
        minute: minN,
        hour: hourN,
        dom: domN,
      };
  }
  return { ...DEFAULT_CRON, preset: "custom" };
}

function buildCronExpr(s: CronState, raw: string): string {
  switch (s.preset) {
    case "hourly":
      return `0 ${s.minute} * ? * *`;
    case "daily":
      return `0 ${s.minute} ${s.hour} ? * *`;
    case "weekly": {
      const ordered = QUARTZ_ORDER.filter((d) => s.days.includes(d as WeekDay));
      const list = ordered.length ? ordered.join(",") : "MON";
      return `0 ${s.minute} ${s.hour} ? * ${list}`;
    }
    case "monthly":
      return `0 ${s.minute} ${s.hour} ${s.dom} * ?`;
    case "custom":
      return raw; // pass-through; the parent stores the live edited string
  }
}

function parseWeekdayList(s: string): WeekDay[] | null {
  if (s === "*") return [...WEEKDAYS];
  const m = s.match(/^([A-Z]+)-([A-Z]+)$/);
  if (m) {
    const a = QUARTZ_ORDER.indexOf(m[1] as (typeof QUARTZ_ORDER)[number]);
    const b = QUARTZ_ORDER.indexOf(m[2] as (typeof QUARTZ_ORDER)[number]);
    if (a < 0 || b < 0) return null;
    const days: WeekDay[] = [];
    let i = a;
    while (true) {
      const dow = QUARTZ_ORDER[i];
      if (WEEKDAYS.includes(dow as WeekDay)) days.push(dow as WeekDay);
      if (i === b) break;
      i = (i + 1) % 7;
    }
    return days.length ? days : null;
  }
  const parts = s.split(",").map((p) => p.trim().toUpperCase());
  const out: WeekDay[] = [];
  for (const p of parts) {
    if (!WEEKDAYS.includes(p as WeekDay)) return null;
    out.push(p as WeekDay);
  }
  return out.length ? out : null;
}

// ============================================================================
//                                helpers
// ============================================================================

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n));
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function parseIntStrict(s: string, lo: number, hi: number): number | null {
  if (!/^-?\d+$/.test(s)) return null;
  const n = parseInt(s, 10);
  if (n < lo || n > hi) return null;
  return n;
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

// Compute the next moment the schedule would fire, in the local zone, for the
// simple presets. Cron parsing for arbitrary expressions would need a real
// cron library, so "custom" is intentionally not handled here.
function nextFire(preset: CronPreset, s: CronState, now = new Date()): Date | null {
  if (preset === "hourly") {
    const d = new Date(now);
    d.setSeconds(0, 0);
    d.setMinutes(s.minute);
    if (d <= now) d.setHours(d.getHours() + 1);
    return d;
  }
  if (preset === "daily") {
    const d = new Date(now);
    d.setHours(s.hour, s.minute, 0, 0);
    if (d <= now) d.setDate(d.getDate() + 1);
    return d;
  }
  if (preset === "weekly") {
    if (s.days.length === 0) return null;
    const dayMap: Record<WeekDay, number> = {
      MON: 1, TUE: 2, WED: 3, THU: 4, FRI: 5, SAT: 6, SUN: 0,
    };
    const targets = new Set(s.days.map((d) => dayMap[d]));
    for (let i = 0; i < 8; i++) {
      const d = new Date(now);
      d.setDate(d.getDate() + i);
      d.setHours(s.hour, s.minute, 0, 0);
      if (targets.has(d.getDay()) && d > now) return d;
    }
    return null;
  }
  if (preset === "monthly") {
    for (let i = 0; i < 12; i++) {
      const year = now.getFullYear();
      const month = now.getMonth() + i;
      const lastDay = new Date(year, month + 1, 0).getDate();
      const d = new Date(year, month, Math.min(s.dom, lastDay), s.hour, s.minute, 0, 0);
      if (d > now) return d;
    }
    return null;
  }
  return null;
}
