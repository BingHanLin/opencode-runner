// React binding for the i18n module. Holds the active language in state,
// persists changes to localStorage, and exposes:
//   useLang() -> { lang, setLang }   for the language switcher
//   useT()    -> (key, params?)       a t() bound to the current language
//
// Keeping the dictionary itself framework-agnostic (in i18n.ts) means pure,
// non-component code (e.g. cronDescribe) can call t(lang, ...) directly without
// touching React.

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  getInitialLang,
  saveLang,
  t as translate,
  type Lang,
  type MessageKey,
} from "./i18n";

interface LangContextValue {
  lang: Lang;
  setLang: (lang: Lang) => void;
}

const LangContext = createContext<LangContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(() => getInitialLang());

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    saveLang(next);
  }, []);

  const value = useMemo(() => ({ lang, setLang }), [lang, setLang]);
  return <LangContext.Provider value={value}>{children}</LangContext.Provider>;
}

export function useLang(): LangContextValue {
  const ctx = useContext(LangContext);
  if (!ctx) throw new Error("useLang must be used within a LanguageProvider");
  return ctx;
}

/** A t() bound to the current language: t(key, params?). */
export function useT(): (
  key: MessageKey,
  params?: Record<string, string | number>,
) => string {
  const { lang } = useLang();
  return useCallback(
    (key: MessageKey, params?: Record<string, string | number>) =>
      translate(lang, key, params),
    [lang],
  );
}
