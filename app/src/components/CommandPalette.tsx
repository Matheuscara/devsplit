import { useEffect, useMemo, useRef, useState } from "react";
import { Search, CornerDownLeft } from "lucide-react";
import {
  ipc,
  type Profiles,
  type Route,
  type Status,
} from "../lib/ipc.ts";
import type { ViewId } from "../App.tsx";
import { cn } from "../lib/cn.ts";

interface Command {
  id: string;
  label: string;
  hint?: string;
  run: () => unknown;
}

interface NavItem {
  id: ViewId;
  label: string;
}

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  status: Status | null;
  routes: Route[];
  profiles: Profiles | null;
  nav: NavItem[];
  onNavigate: (id: ViewId) => void;
  onToggleProxy: () => void;
  onSelectProfile: (name: string) => void;
  onRefresh: () => void;
}

// Subsequence fuzzy match with a contiguity bonus. Returns null on no match.
function fuzzyMatch(query: string, text: string): number | null {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let qi = 0;
  let score = 0;
  let lastIdx = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      score += lastIdx === ti - 1 ? 3 : 1;
      lastIdx = ti;
      qi++;
    }
  }
  return qi === q.length ? score : null;
}

export function CommandPalette({
  open,
  onClose,
  status,
  routes,
  profiles,
  nav,
  onNavigate,
  onToggleProxy,
  onSelectProfile,
  onRefresh,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const commands = useMemo<Command[]>(() => {
    const list: Command[] = [
      {
        id: "proxy",
        label: status?.running ? "Desligar split" : "Ligar split",
        hint: "proxy",
        run: onToggleProxy,
      },
      {
        id: "cert",
        label: "Instalar certificado raiz",
        hint: "certificado",
        run: () => ipc.installCert(),
      },
      {
        id: "reresolve",
        label: "Re-resolver IP do upstream",
        hint: "rede",
        run: () => ipc.reresolveUpstream(),
      },
    ];
    for (const item of nav) {
      list.push({
        id: `nav:${item.id}`,
        label: `Ir para ${item.label}`,
        hint: "navegar",
        run: () => onNavigate(item.id),
      });
    }
    if (profiles) {
      for (const p of profiles.all) {
        list.push({
          id: `profile:${p}`,
          label: `Perfil: ${p}`,
          hint: profiles.active === p ? "ativo" : "perfil",
          run: () => onSelectProfile(p),
        });
      }
    }
    for (const r of routes) {
      if (r.kind !== "local") continue;
      list.push({
        id: `route:${r.prefix}`,
        label: `${r.enabled ? "Desativar" : "Ativar"} rota ${r.prefix}`,
        hint: "rota",
        run: async () => {
          await ipc.toggleRoute(r.prefix, !r.enabled);
          onRefresh();
        },
      });
    }
    return list;
  }, [
    status,
    routes,
    profiles,
    nav,
    onNavigate,
    onToggleProxy,
    onSelectProfile,
    onRefresh,
  ]);

  const filtered = useMemo(() => {
    const scored: Array<{ cmd: Command; score: number }> = [];
    for (const cmd of commands) {
      const score = fuzzyMatch(query, `${cmd.label} ${cmd.hint ?? ""}`);
      if (score !== null) scored.push({ cmd, score });
    }
    scored.sort((a, b) => b.score - a.score);
    return scored.map((x) => x.cmd);
  }, [commands, query]);

  useEffect(() => {
    setSelected(0);
  }, [query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSelected(0);
      const id = requestAnimationFrame(() => inputRef.current?.focus());
      return () => cancelAnimationFrame(id);
    }
  }, [open]);

  if (!open) return null;

  const exec = async (cmd: Command) => {
    onClose();
    await cmd.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected((i) => Math.min(i + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = filtered[selected];
      if (cmd) void exec(cmd);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[14vh]"
      style={{ animation: "devsplit-overlay-in 120ms ease-out" }}
      onMouseDown={onClose}
    >
      <div
        className="w-full max-w-lg overflow-hidden rounded-lg border border-border bg-surface shadow-2xl shadow-black/50"
        style={{ animation: "devsplit-card-in 140ms ease-out" }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2.5 border-b border-border px-3.5">
          <Search size={16} className="shrink-0 text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Buscar ações…"
            className="h-12 w-full bg-transparent text-sm text-text outline-none placeholder:text-muted"
          />
          <kbd className="shrink-0 rounded border border-border px-1.5 py-0.5 text-[10px] text-muted">
            esc
          </kbd>
        </div>
        <ul className="max-h-80 overflow-y-auto p-1.5">
          {filtered.map((cmd, i) => (
            <li key={cmd.id}>
              <button
                onMouseEnter={() => setSelected(i)}
                onClick={() => void exec(cmd)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm",
                  i === selected
                    ? "bg-surface-2 text-text"
                    : "text-muted hover:text-text",
                )}
              >
                <span className="min-w-0 flex-1 truncate text-text">
                  {cmd.label}
                </span>
                {cmd.hint ? (
                  <span className="shrink-0 text-[10px] uppercase tracking-wide text-muted">
                    {cmd.hint}
                  </span>
                ) : null}
                {i === selected ? (
                  <CornerDownLeft size={13} className="shrink-0 text-muted" />
                ) : null}
              </button>
            </li>
          ))}
          {filtered.length === 0 ? (
            <li className="px-3 py-6 text-center text-sm text-muted">
              Nenhuma ação encontrada.
            </li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
