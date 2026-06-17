import { useEffect, useState, type RefObject } from "react";

export interface SectionNavItem {
  /** id of the section element this link jumps to. */
  id: string;
  label: string;
}

/**
 * A sticky side table-of-contents for a scrollable panel. Each item links to a
 * section by element id; clicking scrolls it to the top of the panel (offset by
 * `topOffset` so it clears any sticky toolbar), and the active item tracks the
 * section currently nearest the top as you scroll.
 *
 * `containerRef` must point at the scrolling element (the `.panel`). We read
 * scroll position from it directly rather than using scrollIntoView so the
 * `topOffset` is honored consistently.
 */
export function SectionNav({
  items,
  containerRef,
  topOffset = 12,
}: {
  items: SectionNavItem[];
  containerRef: RefObject<HTMLElement | null>;
  topOffset?: number;
}) {
  const [active, setActive] = useState(items[0]?.id ?? "");

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let raf = 0;
    const recompute = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        // A section is "current" once its top has scrolled to (or past) the
        // line just below the sticky toolbar. The last such section wins.
        const line = container.getBoundingClientRect().top + topOffset + 1;
        let current = items[0]?.id ?? "";
        for (const it of items) {
          const el = document.getElementById(it.id);
          if (!el) continue;
          if (el.getBoundingClientRect().top <= line) current = it.id;
          else break;
        }
        setActive(current);
      });
    };
    recompute();
    container.addEventListener("scroll", recompute, { passive: true });
    window.addEventListener("resize", recompute);
    return () => {
      cancelAnimationFrame(raf);
      container.removeEventListener("scroll", recompute);
      window.removeEventListener("resize", recompute);
    };
  }, [items, containerRef, topOffset]);

  function go(id: string) {
    const container = containerRef.current;
    const el = document.getElementById(id);
    if (!container || !el) return;
    const top =
      el.getBoundingClientRect().top -
      container.getBoundingClientRect().top +
      container.scrollTop -
      topOffset;
    container.scrollTo({ top, behavior: "smooth" });
    setActive(id);
  }

  return (
    <nav className="toc-nav" style={{ top: topOffset }} aria-label="Sections">
      {items.map((it) => (
        <button
          key={it.id}
          type="button"
          className={`toc-link ${active === it.id ? "active" : ""}`}
          onClick={() => go(it.id)}
        >
          {it.label}
        </button>
      ))}
    </nav>
  );
}
