import { MoonIcon, SunIcon } from "./Icon";
import { useT } from "../LanguageProvider";
import type { Theme } from "../theme";

interface Props {
  theme: Theme;
  onToggle: () => void;
}

export function ThemeToggle({ theme, onToggle }: Props) {
  const t = useT();
  const label = theme === "dark" ? t("theme.switchToLight") : t("theme.switchToDark");
  return (
    <button
      className="btn ghost icon"
      onClick={onToggle}
      aria-label={label}
      title={label}
    >
      {theme === "dark" ? <SunIcon size={16} /> : <MoonIcon size={16} />}
    </button>
  );
}
