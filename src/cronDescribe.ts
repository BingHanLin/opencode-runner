// Compact, human-readable label for a Quartz cron expression.
// Matches the preset shapes that ScheduleEditor knows how to build; anything
// outside those falls back to the raw expression so the user can still see
// what they typed.

const QUARTZ_ORDER = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"] as const;
const WEEKDAY_SHORT: Record<string, string> = {
  SUN: "Sun",
  MON: "Mon",
  TUE: "Tue",
  WED: "Wed",
  THU: "Thu",
  FRI: "Fri",
  SAT: "Sat",
};

export function describeCron(expr: string): string {
  const parts = expr.trim().split(/\s+/);
  if (parts.length < 6) return expr;
  const [sec, min, hour, dom, _month, dow] = parts;
  if (sec !== "0") return expr;
  const m = intIn(min, 0, 59);
  if (m == null) return expr;

  // Hourly: hour=*, dom=?, dow=*
  if (hour === "*" && dom === "?" && dow === "*") {
    return `Hourly · :${pad(m)}`;
  }
  const h = intIn(hour, 0, 23);
  if (h == null) return expr;
  const time = `${pad(h)}:${pad(m)}`;

  // Daily: dom=?, dow=*
  if (dom === "?" && dow === "*") {
    return `Daily · ${time}`;
  }
  // Weekly: dom=?, dow=<list|range|*>
  if (dom === "?") {
    const days = parseWeekdays(dow);
    if (days) return `Weekly · ${formatDays(days)} ${time}`;
  }
  // Monthly: dom=N, dow=?
  if (dow === "?") {
    const d = intIn(dom, 1, 31);
    if (d != null) return `Monthly · day ${d} · ${time}`;
  }
  return expr;
}

/** Same idea as describeCron but for the full `schedule` field. */
export function describeSchedule(schedule: string): string {
  if (schedule.startsWith("cron:")) return describeCron(schedule.slice(5));
  if (schedule.startsWith("once:")) {
    const iso = schedule.slice(5);
    const d = new Date(iso);
    if (!Number.isNaN(d.getTime())) {
      try {
        return `Once · ${new Intl.DateTimeFormat(undefined, {
          month: "short",
          day: "2-digit",
          hour: "2-digit",
          minute: "2-digit",
          hour12: false,
        }).format(d)}`;
      } catch {
        /* fall through */
      }
    }
    return `Once · ${iso}`;
  }
  return "Manual";
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

function formatDays(days: string[]): string {
  if (days.length === 7) return "every day";
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
  if (contiguous) {
    return `${WEEKDAY_SHORT[QUARTZ_ORDER[idx[0]]]}–${WEEKDAY_SHORT[QUARTZ_ORDER[idx[idx.length - 1]]]}`;
  }
  return idx.map((i) => WEEKDAY_SHORT[QUARTZ_ORDER[i]]).join(", ");
}

function intIn(s: string, lo: number, hi: number): number | null {
  if (!/^-?\d+$/.test(s)) return null;
  const n = parseInt(s, 10);
  return n >= lo && n <= hi ? n : null;
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}
