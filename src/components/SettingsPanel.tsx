import { useEffect, useMemo, useRef, useState } from "react";
import { api, pickFile } from "../api";
import { useLang, useT } from "../LanguageProvider";
import { LANGS, type Lang } from "../i18n";
import type { BinaryStatus, Settings, StoragePaths } from "../types";
import { FolderIcon } from "./Icon";
import { SectionNav, type SectionNavItem } from "./SectionNav";

interface Props {
  settings: Settings;
  onSave: (settings: Settings) => Promise<void>;
}

export function SettingsPanel({ settings, onSave }: Props) {
  const t = useT();
  const { lang, setLang } = useLang();
  const [draft, setDraft] = useState<Settings>(settings);
  const [status, setStatus] = useState<BinaryStatus | null>(null);
  const [paths, setPaths] = useState<StoragePaths | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  const navItems: SectionNavItem[] = useMemo(
    () => [
      { id: "set-language", label: t("settings.language.section") },
      { id: "set-binary", label: t("settings.binary.section") },
      { id: "set-history", label: t("settings.history.section") },
      { id: "set-storage", label: t("settings.storage.section") },
    ],
    [t],
  );

  useEffect(() => setDraft(settings), [settings]);

  useEffect(() => {
    api.opencodeBinaryStatus().then(setStatus).catch(() => setStatus(null));
  }, [settings]);

  // Fetch storage paths once on mount — they don't change at runtime.
  useEffect(() => {
    api.storagePaths().then(setPaths).catch(() => setPaths(null));
  }, []);

  const dirty =
    (draft.opencode_binary ?? null) !== (settings.opencode_binary ?? null) ||
    (draft.max_run_history ?? null) !== (settings.max_run_history ?? null);

  async function browse() {
    const p = await pickFile();
    if (p) setDraft({ ...draft, opencode_binary: p });
  }

  async function save() {
    setBusy(true);
    setMessage(null);
    try {
      await onSave(draft);
      const next = await api.opencodeBinaryStatus();
      setStatus(next);
      setMessage(t("settings.saved"));
    } catch (e) {
      setMessage(t("settings.saveFailed", { error: String(e) }));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel panel-pad-top" ref={panelRef}>
      <h2 className="content-title" style={{ marginBottom: 12 }}>
        {t("settings.title")}
      </h2>

      <div className="toc-layout">
        <div className="toc-main">
      <section className="section" id="set-language">
        <div className="section-title">{t("settings.language.section")}</div>
        <div className="field">
          <label className="field-label">{t("settings.language.label")}</label>
          <select
            className="select"
            style={{ maxWidth: 220 }}
            value={lang}
            onChange={(e) => setLang(e.target.value as Lang)}
          >
            {LANGS.map((l) => (
              <option key={l.id} value={l.id}>
                {l.label}
              </option>
            ))}
          </select>
          <div className="help" style={{ marginTop: 8 }}>
            {t("settings.language.help")}
          </div>
        </div>
      </section>

      <section className="section" id="set-binary">
        <div className="section-title">{t("settings.binary.section")}</div>
        <div className="field">
          <label className="field-label">
            {t("settings.binary.label")}
          </label>
          <div className="row">
            <input
              className="input grow"
              value={draft.opencode_binary ?? ""}
              placeholder={t("settings.binary.placeholder")}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  opencode_binary: e.target.value || null,
                })
              }
            />
            <button className="btn" onClick={browse}>
              <FolderIcon size={13} />
              {t("btn.browse")}
            </button>
          </div>
          <div className="help" style={{ marginTop: 8 }}>
            {t("settings.binary.help")}
          </div>
        </div>

        {status && (
          <div
            className={`chip ${status.honored_configured ? "success" : "warn"}`}
          >
            {t("settings.binary.resolved", { path: status.resolved_path })}
          </div>
        )}
      </section>

      <section className="section" id="set-history">
        <div className="section-title">{t("settings.history.section")}</div>
        <div className="field">
          <label className="field-label">{t("settings.history.label")}</label>
          <input
            type="number"
            min={0}
            step={1}
            className="input"
            style={{ maxWidth: 160 }}
            value={draft.max_run_history ?? ""}
            placeholder={t("settings.history.placeholder")}
            onChange={(e) => {
              const v = e.target.value;
              if (v === "") return setDraft({ ...draft, max_run_history: null });
              const n = Math.max(0, Math.floor(Number(v)));
              setDraft({ ...draft, max_run_history: Number.isFinite(n) ? n : null });
            }}
          />
          <div className="help" style={{ marginTop: 8 }}>
            {t("settings.history.help")}
          </div>
        </div>
      </section>

      <div className="row" style={{ justifyContent: "flex-end", gap: 8 }}>
        <button
          className="btn"
          disabled={!dirty || busy}
          onClick={() => setDraft(settings)}
        >
          {t("btn.revert")}
        </button>
        <button className="btn primary" disabled={!dirty || busy} onClick={save}>
          {busy ? t("btn.saving") : t("btn.save")}
        </button>
      </div>
      {message && (
        <div className="help" style={{ marginTop: 8, textAlign: "right" }}>
          {message}
        </div>
      )}

      <section className="section" id="set-storage" style={{ marginTop: 14 }}>
        <div className="section-title">{t("settings.storage.section")}</div>
        {paths ? (
          <div className="storage-paths">
            <StoragePathRow
              label={t("settings.storage.config.label")}
              path={paths.config_path}
              note={t("settings.storage.config.note")}
            />
            <StoragePathRow
              label={t("settings.storage.runsdb.label")}
              path={paths.runs_db}
              note={t("settings.storage.runsdb.note")}
            />
            <StoragePathRow
              label={t("settings.storage.sessiondb.label")}
              path={paths.opencode_session_db}
              note={t("settings.storage.sessiondb.note")}
            />
            <StoragePathRow
              label={t("settings.storage.worktree.label")}
              path={paths.worktree_root}
              note={t("settings.storage.worktree.note")}
            />
          </div>
        ) : (
          <div className="help">{t("settings.storage.loading")}</div>
        )}
      </section>
        </div>
        <SectionNav items={navItems} containerRef={panelRef} topOffset={16} />
      </div>
    </div>
  );
}

function StoragePathRow({
  label,
  path,
  note,
}: {
  label: string;
  path: string;
  note: string;
}) {
  return (
    <div className="storage-path-row">
      <div className="storage-path-label">{label}</div>
      <div className="storage-path-value mono" title={path}>
        {path}
      </div>
      <div className="help">{note}</div>
    </div>
  );
}
