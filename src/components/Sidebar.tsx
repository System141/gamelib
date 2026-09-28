// Left navigation: catalog views and stores. Collapses to icons on narrower windows.

import clsx from "clsx";
import { LayoutGrid, Link2, Sparkles } from "lucide-react";
import type { ReactNode } from "react";
import { tr } from "../i18n/tr";
import { formatNumber } from "../lib/format";
import type { AppStatus } from "../lib/types";
import type { View } from "../hooks/useFilters";
import { StoreMark } from "./badges";
import { Logo } from "./icons";

interface Props {
  view: View;
  onChange: (view: View) => void;
  status: AppStatus | undefined;
}

interface Item {
  id: View;
  label: string;
  icon: ReactNode;
  count?: number;
}

export function Sidebar({ view, onChange, status }: Props) {
  const counts = status?.storeCounts;
  const sections: { title: string; items: Item[] }[] = [
    {
      title: tr.nav.discover,
      items: [
        { id: "all", label: tr.views.all, icon: <LayoutGrid size={17} /> },
        { id: "new", label: tr.views.new, icon: <Sparkles size={17} className="text-violet" /> },
        { id: "links", label: tr.views.links, icon: <Link2 size={17} />, count: status?.linkedGameCount || undefined },
      ],
    },
    {
      title: tr.nav.stores,
      items: [
        { id: "gog", label: tr.storeNames.gog, icon: <StoreMark store="gog" size={19} />, count: counts?.gog || undefined },
        { id: "itch", label: tr.storeNames.itch, icon: <StoreMark store="itch" size={19} />, count: counts?.itch || undefined },
      ],
    },
  ];

  return (
    <aside className="glass relative z-30 flex w-16 shrink-0 flex-col border-r border-white/6 xl:w-60" aria-label={tr.nav.label}>
      <div className="flex h-16 shrink-0 items-center justify-center gap-2.5 px-4 xl:justify-start xl:px-5">
        <Logo size={30} />
        <span className="hidden font-display text-lg font-semibold tracking-tight text-ink-50 xl:inline">
          Game<span className="text-gradient">Lib</span>
        </span>
      </div>
      <nav className="scrollbar-none flex-1 overflow-y-auto px-2 pt-2 pb-4 xl:px-3">
        {sections.map((section, i) => (
          <div key={section.title} className={clsx(i > 0 && "mt-5")}>
            <div className="hidden px-3 pb-1.5 text-[11px] font-semibold tracking-wider text-ink-500 uppercase xl:block">
              {section.title}
            </div>
            {i > 0 && <div className="mx-auto mb-3 h-px w-7 bg-white/8 xl:hidden" />}
            <ul className="space-y-0.5">
              {section.items.map((item) => (
                <li key={item.id}>
                  <NavItem item={item} active={view === item.id} onClick={() => onChange(item.id)} />
                </li>
              ))}
            </ul>
          </div>
        ))}
      </nav>
    </aside>
  );
}

function NavItem({ item, active, onClick }: { item: Item; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      title={item.label}
      className={clsx(
        "relative flex h-10 w-full items-center justify-center gap-3 rounded-lg px-3 text-[13.5px] font-medium transition xl:justify-start",
        active ? "bg-white/8 text-white ring-1 ring-white/10" : "text-ink-300 hover:bg-white/4 hover:text-ink-50",
      )}
    >
      {active && <span className="absolute top-2.5 bottom-2.5 left-0 w-0.5 rounded-full bg-accent" />}
      <span className={clsx("grid size-5 shrink-0 place-items-center", !active && "text-ink-400")}>{item.icon}</span>
      <span className="hidden min-w-0 flex-1 truncate text-left xl:block">{item.label}</span>
      {item.count != null && (
        <span className={clsx("hidden text-xs tabular-nums xl:inline", active ? "text-ink-200" : "text-ink-500")}>
          {formatNumber(item.count)}
        </span>
      )}
    </button>
  );
}
