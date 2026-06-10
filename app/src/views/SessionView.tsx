import { useEffect, useRef, useState } from "react";
import { KeyRound, ShieldAlert, Clock } from "lucide-react";
import { ipc, type TrafficEntry } from "../lib/ipc.ts";
import { decodeJwt, type JwtDecoded } from "../lib/jwt.ts";
import { cn } from "../lib/cn.ts";
import { Card, CardHeader } from "../components/Card.tsx";
import { Badge } from "../components/Badge.tsx";

const POLL_MS = 1500;

function claimValue(v: unknown): string {
  if (Array.isArray(v)) return v.join(", ");
  if (v !== null && typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function asList(v: unknown): string[] {
  if (Array.isArray(v)) return v.map((x) => String(x));
  if (typeof v === "string") return v.split(/[\s,]+/).filter(Boolean);
  return [];
}

function formatRemaining(ms: number): string {
  if (ms <= 0) return "expirado";
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return m > 0 ? `${m}m ${s}s` : `${s}s`;
}

export function SessionView() {
  const [decoded, setDecoded] = useState<JwtDecoded | null>(null);
  const [unauthorized, setUnauthorized] = useState<TrafficEntry[]>([]);
  const [now, setNow] = useState(() => Date.now());
  const latestIdRef = useRef<number | null>(null);
  const rawRef = useRef<string | null>(null);

  // Track newest request id + collect 401/403 from the live stream.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await ipc.onTraffic((e) => {
        latestIdRef.current = e.id;
        if (e.status === 401 || e.status === 403) {
          setUnauthorized((prev) => [e, ...prev].slice(0, 20));
        }
      });
    })();
    return () => unlisten?.();
  }, []);

  // Poll the latest request's detail for a Bearer token and decode it.
  useEffect(() => {
    let active = true;
    let busy = false;
    const tick = async () => {
      if (busy) return;
      const id = latestIdRef.current;
      if (id === null) return;
      busy = true;
      try {
        const detail = await ipc.getRequestDetail(id);
        if (!active) return;
        const auth = detail.reqHeaders.find(
          ([k]) => k.toLowerCase() === "authorization",
        );
        if (!auth) return;
        const match = auth[1].match(/^Bearer\s+(.+)$/i);
        const token = match ? match[1] : null;
        if (token && token !== rawRef.current) {
          const dec = decodeJwt(token);
          if (dec) {
            rawRef.current = token;
            setDecoded(dec);
          }
        }
      } finally {
        busy = false;
      }
    };
    void tick();
    const handle = setInterval(tick, POLL_MS);
    return () => {
      active = false;
      clearInterval(handle);
    };
  }, []);

  // Drive the expiry countdown.
  useEffect(() => {
    const handle = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(handle);
  }, []);

  if (!decoded) {
    return (
      <div className="flex h-full flex-col">
        <header className="border-b border-border px-6 py-4">
          <h1 className="text-lg font-semibold text-text">Sessão</h1>
          <p className="mt-0.5 text-xs text-muted">
            Inspeção do token Bearer capturado no tráfego ao vivo
          </p>
        </header>
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center text-muted">
          <KeyRound size={32} className="opacity-40" />
          <div>
            <p className="text-sm text-text">Nenhum token visto ainda</p>
            <p className="mt-1 text-xs">
              Quando uma requisição passar com{" "}
              <code className="font-mono text-text/80">Authorization: Bearer</code>
              , o JWT aparece aqui decodificado.
            </p>
          </div>
        </div>
      </div>
    );
  }

  const payload = decoded.payload;
  const alg = String(decoded.header.alg ?? "—");
  const sub = payload.sub !== undefined ? String(payload.sub) : null;
  const name = payload.name !== undefined ? String(payload.name) : null;
  const roles = asList(payload.roles ?? payload.role);
  const scopes = asList(payload.scope ?? payload.scopes);
  const exp = typeof payload.exp === "number" ? payload.exp : null;
  const remainingMs = exp !== null ? exp * 1000 - now : null;

  let expTone: "ok" | "warn" | "danger" = "ok";
  if (remainingMs !== null) {
    if (remainingMs <= 0) expTone = "danger";
    else if (remainingMs < 5 * 60 * 1000) expTone = "warn";
  }
  const expClass =
    expTone === "danger"
      ? "text-danger"
      : expTone === "warn"
        ? "text-warn"
        : "text-accent";

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <header className="flex items-center gap-3 border-b border-border px-6 py-4">
        <div>
          <h1 className="text-lg font-semibold text-text">Sessão</h1>
          <p className="mt-0.5 text-xs text-muted">
            Token Bearer mais recente · alg{" "}
            <span className="font-mono text-text/80">{alg}</span>
          </p>
        </div>
        {remainingMs !== null ? (
          <div className="ml-auto flex items-center gap-2">
            <Clock size={15} className={expClass} />
            <span className={cn("font-mono text-sm font-semibold", expClass)}>
              {remainingMs <= 0
                ? "expirado"
                : `expira em ${formatRemaining(remainingMs)}`}
            </span>
          </div>
        ) : null}
      </header>

      <div className="flex flex-col gap-4 p-6">
        <div className="grid grid-cols-2 gap-4">
          <Card>
            <CardHeader title="Usuário" />
            <div className="px-4 py-3">
              <p className="font-mono text-sm text-text">
                {name ?? sub ?? "—"}
              </p>
              {sub ? (
                <p className="mt-1 text-xs text-muted">
                  sub <span className="font-mono">{sub}</span>
                </p>
              ) : null}
            </div>
          </Card>
          <Card>
            <CardHeader title="Expiração" />
            <div className="px-4 py-3">
              <p className={cn("font-mono text-sm", expClass)}>
                {remainingMs === null
                  ? "sem claim exp"
                  : remainingMs <= 0
                    ? "token expirado"
                    : formatRemaining(remainingMs)}
              </p>
              {exp !== null ? (
                <p className="mt-1 text-xs text-muted">
                  {new Date(exp * 1000).toLocaleString("pt-BR")}
                </p>
              ) : null}
            </div>
          </Card>
        </div>

        {roles.length > 0 || scopes.length > 0 ? (
          <Card>
            <CardHeader title="Permissões" />
            <div className="flex flex-col gap-3 px-4 py-3">
              {roles.length > 0 ? (
                <div>
                  <p className="mb-1.5 text-xs uppercase tracking-wide text-muted">
                    Roles
                  </p>
                  <div className="flex flex-wrap gap-1.5">
                    {roles.map((r) => (
                      <Badge key={r} tone="local">
                        {r}
                      </Badge>
                    ))}
                  </div>
                </div>
              ) : null}
              {scopes.length > 0 ? (
                <div>
                  <p className="mb-1.5 text-xs uppercase tracking-wide text-muted">
                    Scopes
                  </p>
                  <div className="flex flex-wrap gap-1.5">
                    {scopes.map((s) => (
                      <Badge key={s} tone="neutral">
                        {s}
                      </Badge>
                    ))}
                  </div>
                </div>
              ) : null}
            </div>
          </Card>
        ) : null}

        <Card>
          <CardHeader title="Claims" subtitle="Payload decodificado do JWT" />
          <table className="w-full text-sm">
            <tbody className="divide-y divide-border">
              {Object.entries(payload).map(([k, v]) => (
                <tr key={k}>
                  <td className="w-40 px-4 py-2 align-top font-mono text-xs text-muted">
                    {k}
                  </td>
                  <td className="px-4 py-2 break-all font-mono text-xs text-text">
                    {k === "exp" || k === "iat" || k === "nbf"
                      ? `${claimValue(v)} · ${new Date(Number(v) * 1000).toLocaleString("pt-BR")}`
                      : claimValue(v)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card>
          <CardHeader
            title="Requisições negadas"
            subtitle={`${unauthorized.length} resposta(s) 401/403 no stream`}
          />
          {unauthorized.length === 0 ? (
            <div className="flex items-center gap-2 px-4 py-4 text-sm text-muted">
              <ShieldAlert size={15} className="opacity-50" />
              Nenhuma rejeição de auth observada.
            </div>
          ) : (
            <ul className="divide-y divide-border">
              {unauthorized.map((e) => (
                <li
                  key={e.id}
                  className="flex items-center gap-3 px-4 py-2 font-mono text-xs"
                >
                  <Badge tone={e.status === 403 ? "danger" : "warn"}>
                    {e.status}
                  </Badge>
                  <span className="text-muted">{e.method}</span>
                  <span className="min-w-0 flex-1 truncate text-text">
                    {e.path}
                  </span>
                  <span className="shrink-0 text-muted">
                    {new Date(e.ts).toLocaleTimeString("pt-BR", {
                      hour12: false,
                    })}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>
    </div>
  );
}
