import { useEffect, useMemo, useRef, useState } from "react";
import {
  Pause,
  Play,
  Trash2,
  Search,
  X,
  Copy,
  Download,
  Check,
} from "lucide-react";
import {
  ipc,
  type TrafficEntry,
  type RequestDetail,
} from "../lib/ipc.ts";
import { cn } from "../lib/cn.ts";
import { Button } from "../components/Button.tsx";
import { Badge } from "../components/Badge.tsx";
import { Card, CardHeader } from "../components/Card.tsx";
import { toCurl, toHar, downloadBlob } from "../lib/export.ts";

const MAX_ROWS = 500;

type StatusClass = "all" | "2xx" | "4xx" | "5xx";

const STATUS_FILTERS: Array<{ id: StatusClass; label: string }> = [
  { id: "all", label: "Tudo" },
  { id: "2xx", label: "2xx" },
  { id: "4xx", label: "4xx" },
  { id: "5xx", label: "5xx" },
];

const METHOD_TONE: Record<string, string> = {
  GET: "text-[#7cb0ff]",
  POST: "text-accent",
  PUT: "text-warn",
  PATCH: "text-warn",
  DELETE: "text-danger",
};

function statusTone(status?: number): "ok" | "warn" | "danger" | "neutral" {
  if (status === undefined) return "neutral";
  if (status < 400) return "ok";
  if (status < 500) return "warn";
  return "danger";
}

function inStatusClass(filter: StatusClass, status?: number): boolean {
  if (filter === "all") return true;
  if (status === undefined) return false;
  if (filter === "2xx") return status >= 200 && status < 300;
  if (filter === "4xx") return status >= 400 && status < 500;
  return status >= 500;
}

function formatSize(bytes?: number): string {
  if (bytes === undefined) return "—";
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[idx];
}

function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) {
    return <span className="text-[10px] text-muted">—</span>;
  }
  const w = 72;
  const h = 18;
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const points = values
    .map((v, i) => {
      const x = (i / (values.length - 1)) * w;
      const y = h - ((v - min) / span) * (h - 2) - 1;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg width={w} height={h} className="text-accent/70">
      <polyline
        points={points}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
    </svg>
  );
}

interface PrefixAgg {
  prefix: string;
  local: number[];
  passthrough: number[];
  recent: number[];
}

function LatencyPanel({ events }: { events: TrafficEntry[] }) {
  const rows = useMemo<PrefixAgg[]>(() => {
    const byPrefix = new Map<string, PrefixAgg>();
    // events arrive newest-first; build recent[] chronologically.
    for (let i = events.length - 1; i >= 0; i--) {
      const e = events[i];
      if (e.latencyMs === undefined) continue;
      const seg = e.path.split("/")[1] ?? "";
      const key = seg ? `/${seg}` : "/";
      let bucket = byPrefix.get(key);
      if (!bucket) {
        bucket = { prefix: key, local: [], passthrough: [], recent: [] };
        byPrefix.set(key, bucket);
      }
      if (e.decision === "local") bucket.local.push(e.latencyMs);
      else bucket.passthrough.push(e.latencyMs);
      bucket.recent.push(e.latencyMs);
    }
    const list = [...byPrefix.values()];
    list.sort(
      (a, b) =>
        b.local.length + b.passthrough.length - (a.local.length + a.passthrough.length),
    );
    return list.slice(0, 6);
  }, [events]);

  if (rows.length === 0) return null;

  return (
    <Card>
      <CardHeader
        title="Latência por rota"
        subtitle="p50 / p95 por prefixo, separando local e passthrough"
      />
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-xs uppercase tracking-wide text-muted">
            <th className="px-4 py-2 font-medium">Prefixo</th>
            <th className="px-3 py-2 font-medium text-right">Local p50/p95</th>
            <th className="px-3 py-2 font-medium text-right">Pass p50/p95</th>
            <th className="px-3 py-2 font-medium text-right">Req</th>
            <th className="px-4 py-2 font-medium text-right">Tendência</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border">
          {rows.map((r) => (
            <tr key={r.prefix} className="font-mono">
              <td className="px-4 py-1.5 text-text">{r.prefix}</td>
              <td className="px-3 py-1.5 text-right text-accent">
                {r.local.length
                  ? `${percentile(r.local, 50)} / ${percentile(r.local, 95)} ms`
                  : "—"}
              </td>
              <td className="px-3 py-1.5 text-right text-[#7cb0ff]">
                {r.passthrough.length
                  ? `${percentile(r.passthrough, 50)} / ${percentile(r.passthrough, 95)} ms`
                  : "—"}
              </td>
              <td className="px-3 py-1.5 text-right text-muted">
                {r.local.length + r.passthrough.length}
              </td>
              <td className="px-4 py-1.5">
                <div className="flex justify-end">
                  <Sparkline values={r.recent.slice(-24)} />
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </Card>
  );
}

function HeaderTable({ headers }: { headers: Array<[string, string]> }) {
  return (
    <table className="w-full text-xs">
      <tbody className="divide-y divide-border">
        {headers.map(([k, v], i) => (
          <tr key={`${k}-${i}`}>
            <td className="w-44 px-3 py-1.5 align-top font-mono text-muted">
              {k}
            </td>
            <td className="px-3 py-1.5 break-all font-mono text-text">{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function DetailDrawer({
  detail,
  onClose,
}: {
  detail: RequestDetail | null;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<"request" | "response">("request");
  const [raw, setRaw] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setTab("request");
    setRaw(false);
    setCopied(false);
  }, [detail?.id]);

  const copyCurl = async () => {
    if (!detail) return;
    await navigator.clipboard.writeText(toCurl(detail));
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const exportHar = () => {
    if (!detail) return;
    downloadBlob(
      `devsplit-${detail.id}.har`,
      JSON.stringify(toHar(detail), null, 2),
      "application/json",
    );
  };

  const body = detail
    ? tab === "request"
      ? detail.reqBody
      : detail.respBody
    : undefined;
  const headers = detail
    ? tab === "request"
      ? detail.reqHeaders
      : detail.respHeaders
    : [];

  return (
    <div
      className="fixed inset-0 z-40 flex justify-end bg-black/40"
      style={{ animation: "devsplit-overlay-in 120ms ease-out" }}
      onMouseDown={onClose}
    >
      <div
        className="flex h-full w-full max-w-xl flex-col border-l border-border bg-surface"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {detail === null ? (
          <div className="flex flex-1 items-center justify-center text-sm text-muted">
            Carregando detalhe…
          </div>
        ) : (
          <>
            <header className="flex items-start gap-3 border-b border-border px-5 py-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "text-xs font-semibold",
                      METHOD_TONE[detail.method] ?? "text-muted",
                    )}
                  >
                    {detail.method}
                  </span>
                  <Badge
                    tone={
                      detail.decision === "local" ? "local" : "passthrough"
                    }
                  >
                    {detail.decision}
                  </Badge>
                  {detail.redacted ? (
                    <Badge tone="neutral">redatado</Badge>
                  ) : null}
                </div>
                <p className="mt-1 break-all font-mono text-sm text-text">
                  {detail.host}
                  {detail.path}
                </p>
              </div>
              <button
                onClick={onClose}
                className="ml-auto shrink-0 rounded-md p-1 text-muted hover:bg-surface-2 hover:text-text"
                aria-label="Fechar"
              >
                <X size={16} />
              </button>
            </header>

            <div className="grid grid-cols-4 gap-px border-b border-border bg-border text-xs">
              <div className="bg-surface px-4 py-2">
                <p className="text-muted">Status</p>
                <Badge tone={statusTone(detail.status)}>
                  {detail.status ?? "—"}
                </Badge>
              </div>
              <div className="bg-surface px-4 py-2">
                <p className="text-muted">Latência</p>
                <p className="font-mono text-text">
                  {detail.latencyMs !== undefined
                    ? `${detail.latencyMs} ms`
                    : "—"}
                </p>
              </div>
              <div className="bg-surface px-4 py-2">
                <p className="text-muted">Req</p>
                <p className="font-mono text-text">
                  {formatSize(detail.reqSize)}
                </p>
              </div>
              <div className="bg-surface px-4 py-2">
                <p className="text-muted">Resp</p>
                <p className="font-mono text-text">
                  {formatSize(detail.respSize)}
                </p>
              </div>
            </div>

            <div className="flex items-center gap-2 border-b border-border px-5 py-2.5">
              <Button size="sm" variant="default" onClick={copyCurl}>
                {copied ? <Check size={13} /> : <Copy size={13} />}
                {copied ? "Copiado" : "Copiar como cURL"}
              </Button>
              <Button size="sm" variant="default" onClick={exportHar}>
                <Download size={13} />
                Exportar HAR
              </Button>
            </div>

            <div className="flex items-center gap-1 border-b border-border px-5 py-2">
              {(["request", "response"] as const).map((t) => (
                <button
                  key={t}
                  onClick={() => setTab(t)}
                  className={cn(
                    "rounded-md px-2.5 py-1 text-xs font-medium capitalize",
                    tab === t
                      ? "bg-surface-2 text-text"
                      : "text-muted hover:text-text",
                  )}
                >
                  {t === "request" ? "Request" : "Response"}
                </button>
              ))}
              <button
                onClick={() => setRaw((v) => !v)}
                className="ml-auto rounded-md px-2 py-1 text-xs text-muted hover:text-text"
              >
                {raw ? "JSON" : "raw"}
              </button>
            </div>

            <div className="flex-1 overflow-y-auto">
              <div className="px-5 py-3">
                <p className="mb-1.5 text-xs uppercase tracking-wide text-muted">
                  Headers
                </p>
                <HeaderTable headers={headers} />
              </div>
              <div className="px-5 pb-5">
                <p className="mb-1.5 text-xs uppercase tracking-wide text-muted">
                  Body
                </p>
                {body ? (
                  <pre className="overflow-x-auto rounded-md border border-border bg-bg p-3 font-mono text-xs leading-relaxed text-text">
                    {raw ? body : prettyJson(body)}
                  </pre>
                ) : (
                  <p className="text-xs text-muted">Sem corpo.</p>
                )}
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export function TrafficView() {
  const [events, setEvents] = useState<TrafficEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusClass>("all");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<RequestDetail | null>(null);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await ipc.onTraffic((e) => {
        if (pausedRef.current) return;
        setEvents((prev) => {
          const next = [e, ...prev];
          return next.length > MAX_ROWS ? next.slice(0, MAX_ROWS) : next;
        });
      });
    })();
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (selectedId === null) {
      setDetail(null);
      return;
    }
    let active = true;
    setDetail(null);
    (async () => {
      const d = await ipc.getRequestDetail(selectedId);
      if (active) setDetail(d);
    })();
    return () => {
      active = false;
    };
  }, [selectedId]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return events.filter((e) => {
      if (!inStatusClass(statusFilter, e.status)) return false;
      if (!q) return true;
      return (
        e.path.toLowerCase().includes(q) || e.method.toLowerCase().includes(q)
      );
    });
  }, [events, query, statusFilter]);
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="border-b border-border px-6 py-4">
        <div className="flex items-center gap-3">
          <div>
            <h1 className="text-lg font-semibold text-text">Inspector</h1>
            <p className="mt-0.5 text-xs text-muted">
              {filtered.length} de {events.length} · janela de {MAX_ROWS}
            </p>
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPaused((v) => !v)}>
              {paused ? <Play size={14} /> : <Pause size={14} />}
              {paused ? "Retomar" : "Pausar"}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEvents([])}>
              <Trash2 size={14} />
              Limpar
            </Button>
          </div>
        </div>
        <div className="mt-3 flex items-center gap-3">
          <div className="flex h-8 flex-1 items-center gap-2 rounded-md border border-border bg-surface-2 px-2.5">
            <Search size={14} className="shrink-0 text-muted" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filtrar por caminho ou método…"
              className="w-full bg-transparent text-sm text-text outline-none placeholder:text-muted"
            />
            {query ? (
              <button
                onClick={() => setQuery("")}
                className="shrink-0 text-muted hover:text-text"
                aria-label="Limpar busca"
              >
                <X size={13} />
              </button>
            ) : null}
          </div>
          <div className="flex items-center gap-1 rounded-md border border-border bg-surface-2 p-0.5">
            {STATUS_FILTERS.map((f) => (
              <button
                key={f.id}
                onClick={() => setStatusFilter(f.id)}
                className={cn(
                  "rounded px-2.5 py-1 text-xs font-medium transition-colors",
                  statusFilter === f.id
                    ? "bg-bg text-text"
                    : "text-muted hover:text-text",
                )}
              >
                {f.label}
              </button>
            ))}
          </div>
        </div>
      </header>

      <div className="flex-1 overflow-y-auto">
        <div className="p-6 pb-0">
          <LatencyPanel events={events} />
        </div>
        <table className="mt-4 w-full text-sm">
          <thead className="sticky top-0 bg-bg">
            <tr className="text-left text-xs uppercase tracking-wide text-muted">
              <th className="px-6 py-2 font-medium">Hora</th>
              <th className="px-3 py-2 font-medium">Método</th>
              <th className="px-3 py-2 font-medium">Host</th>
              <th className="px-3 py-2 font-medium">Caminho</th>
              <th className="px-3 py-2 font-medium">Decisão</th>
              <th className="px-3 py-2 font-medium text-right">Status</th>
              <th className="px-3 py-2 font-medium text-right">Latência</th>
              <th className="px-6 py-2 font-medium text-right">Tamanho</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {filtered.map((e) => (
              <tr
                key={e.id}
                onClick={() => setSelectedId(e.id)}
                className={cn(
                  "cursor-pointer font-mono transition-colors hover:bg-surface-2/40",
                  selectedId === e.id && "bg-surface-2/60",
                )}
              >
                <td className="whitespace-nowrap px-6 py-1.5 text-xs text-muted">
                  {new Date(e.ts).toLocaleTimeString("pt-BR", { hour12: false })}
                </td>
                <td
                  className={cn(
                    "px-3 py-1.5 text-xs font-semibold",
                    METHOD_TONE[e.method] ?? "text-muted",
                  )}
                >
                  {e.method}
                </td>
                <td className="px-3 py-1.5 text-xs text-muted">{e.host}</td>
                <td className="max-w-0 truncate px-3 py-1.5 text-text">
                  {e.path}
                </td>
                <td className="px-3 py-1.5">
                  <Badge tone={e.decision}>{e.decision}</Badge>
                </td>
                <td className="px-3 py-1.5 text-right">
                  <Badge tone={statusTone(e.status)}>{e.status ?? "—"}</Badge>
                </td>
                <td className="px-3 py-1.5 text-right text-xs text-muted">
                  {e.latencyMs !== undefined ? `${e.latencyMs} ms` : "—"}
                </td>
                <td className="px-6 py-1.5 text-right text-xs text-muted">
                  {formatSize(e.respSize)}
                </td>
              </tr>
            ))}
            {filtered.length === 0 ? (
              <tr>
                <td colSpan={8} className="px-6 py-10 text-center text-muted">
                  {events.length === 0
                    ? "Aguardando tráfego…"
                    : "Nenhuma requisição corresponde ao filtro."}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>

      {selectedId !== null ? (
        <DetailDrawer detail={detail} onClose={() => setSelectedId(null)} />
      ) : null}
    </div>
  );
}
