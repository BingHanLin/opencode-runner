// Human-readable label for a Quartz cron expression.
//
// Two tiers:
//   1. Compact preset labels ("Daily · 09:00") for the shapes ScheduleEditor's
//      wizard builds — short enough to sit in a chip. Pulled from the i18n
//      dictionary so they localize with the rest of the UI.
//   2. Anything else (lists like 9,18, ranges, steps, seconds) is handed to
//      cronstrue, which has its own locale support.
//
// `lang` defaults to English. Call sites (ScheduleChip / ScheduleEditor) pass
// the active language from the LanguageProvider; non-React callers can pass a
// Lang directly since this module stays framework-agnostic.

import cronstrue from "cronstrue/i18n";
import { cronLocale, t, type Lang, type MessageKey } from "./i18n";

const QUARTZ_ORDER = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"] as const;
const WEEKDAY_KEY: Record<string, MessageKey> = {
  SUN: "wd.sun",
  MON: "wd.mon",
  TUE: "wd.tue",
  WED: "wd.wed",
  THU: "wd.thu",
  FRI: "wd.fri",
  SAT: "wd.sat",
};

export function describeCron(expr: string, lang: Lang = "en"): string {
  return compactCronLabel(expr, lang) ?? verboseCron(expr, lang);
}

// Tier 1: the compact "Daily · 09:00" style labels for the wizard's preset
// shapes. Returns null when the expression doesn't match a preset, so the
// caller can fall through to cronstrue.
function compactCronLabel(expr: string, lang: Lang): string | null {
  const parts = expr.trim().split(/\s+/);
  if (parts.length < 6) return null;
  const [sec, min, hour, dom, _month, dow] = parts;
  if (sec !== "0") return null;
  const m = intIn(min, 0, 59);
  if (m == null) return null;

  // Hourly: hour=*, dom=?, dow=*
  if (hour === "*" && dom === "?" && dow === "*") {
    return t(lang, "crondesc.hourly", { min: pad(m) });
  }
  const h = intIn(hour, 0, 23);
  if (h == null) return null;
  const time = `${pad(h)}:${pad(m)}`;

  // Daily: dom=?, dow=*
  if (dom === "?" && dow === "*") {
    return t(lang, "crondesc.daily", { time });
  }
  // Weekly: dom=?, dow=<list|range|*>
  if (dom === "?") {
    const days = parseWeekdays(dow);
    if (days) return t(lang, "crondesc.weekly", { days: formatDays(days, lang), time });
  }
  // Monthly: dom=N, dow=?
  if (dow === "?") {
    const d = intIn(dom, 1, 31);
    if (d != null) return t(lang, "crondesc.monthly", { day: d, time });
  }
  return null;
}

// Tier 2: full-sentence description for arbitrary expressions (lists, ranges,
// steps, seconds). Falls back to the raw expression if cronstrue can't parse
// it, so the user always sees something.
function verboseCron(expr: string, lang: Lang): string {
  try {
    return cronstrue.toString(expr, {
      locale: cronLocale(lang),
      use24HourTimeFormat: true,
    });
  } catch {
    return expr;
  }
}

/** Same idea as describeCron but for the full `schedule` field. */
export function describeSchedule(schedule: string, lang: Lang = "en"): string {
  if (schedule.startsWith("cron:")) return describeCron(schedule.slice(5), lang);
  if (schedule.startsWith("once:")) {
    const iso = schedule.slice(5);
    const d = new Date(iso);
    if (!Number.isNaN(d.getTime())) {
      try {
        return t(lang, "crondesc.once", {
          when: new Intl.DateTimeFormat(undefined, {
            month: "short",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
            hour12: false,
          }).format(d),
        });
      } catch {
        /* fall through */
      }
    }
    return t(lang, "crondesc.once", { when: iso });
  }
  return t(lang, "crondesc.manual");
}

function parseWeekdays(s: string): string[] | null {
  if (s === "*") return [...QUARTZ_ORDER];
  const range = s.match(/^([A-Z]+)-([A-Z]+)$/);
  if (range) {
    const a = QUARTZ_ORDER.indexOf(range[1] as (typeof QUARTZ_ORDER)[number]);
    const b = QUARTZ_ORDER.indexOf(range[2] as (typeof QUARTZ_ORDER)[number]);
    if (a < 0 || b < 0) return null;
    const out: string[] = [];
    let i = a;
    while (true) {
      out.push(QUARTZ_ORDER[i]);
      if (i === b) break;
      i = (i + 1) % 7;
    }
    return out;
  }
  const list = s.split(",").map((p) => p.trim().toUpperCase());
  for (const p of list) {
    if (!QUARTZ_ORDER.includes(p as (typeof QUARTZ_ORDER)[number])) return null;
  }
  return list;
}

function formatDays(days: string[], lang: Lang): string {
  if (days.length === 7) return t(lang, "crondesc.everyDay");
  // Detect contiguous range in Quartz order (SUN..SAT) so MON-FRI renders as
  // "Mon–Fri" instead of "Mon, Tue, Wed, Thu, Fri".
  const idx = days
    .map((d) => QUARTZ_ORDER.indexOf(d as (typeof QUARTZ_ORDER)[number]))
    .sort((a, b) => a - b);
  let contiguous = idx.length > 1;
  for (let i = 1; i < idx.length; i++) {
    if (idx[i] !== idx[i - 1] + 1) {
      contiguous = false;
      break;
    }
  }
  const name = (i: number) => t(lang, WEEKDAY_KEY[QUARTZ_ORDER[i]]);
  if (contiguous) {
    return `${name(idx[0])}${t(lang, "crondesc.rangeSep")}${name(idx[idx.length - 1])}`;
  }
  return idx.map(name).join(t(lang, "crondesc.listSep"));
}

function intIn(s: string, lo: number, hi: number): number | null {
  if (!/^-?\d+$/.test(s)) return null;
  const n = parseInt(s, 10);
  return n >= lo && n <= hi ? n : null;
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}
