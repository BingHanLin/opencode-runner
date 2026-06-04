import { describeSchedule } from "../cronDescribe";

interface Props {
  status: string;
  label?: string;
}

export function StatusChip({ status, label }: Props) {
  const tone = chipTone(status);
  return <span className={`chip ${tone}`}>{label ?? status}</span>;
}

function chipTone(status: string): string {
  switch (status) {
    case "ok":
      return "success";
    case "running":
      return "info";
    case "error":
      return "error";
    case "aborted":
      return "warn";
    default:
      return "";
  }
}

export function ScheduleChip({ schedule }: { schedule: string }) {
  let tone = "";
  if (schedule.startsWith("cron:")) tone = "accent";
  else if (schedule.startsWith("once:")) tone = "info";
  const label = describeSchedule(schedule);
  return (
    <span className={`chip ${tone}`} title={schedule}>
      {label}
    </span>
  );
}
