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
  let label: string;
  let tone = "";
  if (schedule.startsWith("cron:")) {
    label = schedule.slice(5);
    tone = "accent";
  } else if (schedule.startsWith("once:")) {
    label = `once · ${schedule.slice(5)}`;
    tone = "info";
  } else {
    label = "manual";
  }
  return <span className={`chip ${tone}`}>{label}</span>;
}
