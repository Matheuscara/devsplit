import { useState } from "react";
import {
  CheckCircle2,
  AlertTriangle,
  Plus,
  Power,
  RefreshCw,
  Trash2,
  Radar,
  Server,
} from "lucide-react";
import {
  ipc,
  type DoctorCheck,
  type Profiles,
  type Route,
  type Status,
  type LocalService,
} from "../lib/ipc.ts";
import { cn } from "../lib/cn.ts";
import { Button } from "../components/Button.tsx";
import { Badge } from "../components/Badge.tsx";
import { Switch } from "../components/Switch.tsx";
import { Card, CardHeader } from "../components/Card.tsx";

interface RoutesViewProps {
  status: Status | null;
  routes: Route[];
  doctor: DoctorCheck[];
  profiles: Profiles | null;
  onToggleProxy: () => void;
  onSelectProfile: (name: string) => void;
  onRoutesChanged: () => void;
  onRerunDoctor: () => void;
}

export function RoutesView({
  status,
  routes,
  doctor,
  profiles,
  onToggleProxy,
  onSelectProfile,
  onRoutesChanged,
  onRerunDoctor,
}: RoutesViewProps) {
  const [adding, setAdding] = useState(false);
  const [prefix, setPrefix] = useState("");
  const [target, setTarget] = useState("");
  const [detected, setDetected] = useState<LocalService[] | null>(null);
  const [detecting, setDetecting] = useState(false);

  const running = status?.running ?? false;
  const warnings = doctor.filter((c) => !c.ok).length;

  const submitAdd = async () => {
    const p = prefix.trim();
    const t = target.trim();
    if (!p || !t) return;
    await ipc.addRoute(p.startsWith("/") ? p : `/${p}`, t);
    setPrefix("");
    setTarget("");
    setAdding(false);
    onRoutesChanged();
  };

  const detectServices = async () => {
    setDetecting(true);
    try {
      setDetected(await ipc.detectServices());
    } finally {
      setDetecting(false);
    }
  };

  const fillFromService = (svc: LocalService) => {
    const slug = svc.hint
      ? svc.hint.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "")
      : "";
    setPrefix(`/${slug || `porta-${svc.port}`}`);
    setTarget(`http://127.0.0.1:${svc.port}`);
    setAdding(true);
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      {/* Header */}
      <header className="flex items-center gap-4 border-b border-border px-6 py-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-xs text-muted">
            <span>Domínio interceptado</span>
          </div>
          <h1 className="truncate font-mono text-lg font-semibold text-text">
            {status?.interceptHost ?? "—"}
          </h1>
          <p className="mt-0.5 text-xs text-muted">
            escutando em{" "}
            <span className="font-mono text-text/80">
              {status?.listenAddr ?? "—"}
            </span>
          </p>
          {status && status.hosts.length > 1 ? (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              {status.hosts.map((h) => (
                <span
                  key={h}
                  className="inline-flex items-center gap-1 rounded-md border border-border bg-surface-2 px-1.5 py-0.5 font-mono text-[11px] text-muted"
                >
                  <Server size={11} />
                  {h}
                </span>
              ))}
            </div>
          ) : null}
        </div>

        <div className="ml-auto flex items-center gap-3">
          {profiles ? (
            <label className="flex items-center gap-2 text-xs text-muted">
              Perfil
              <select
                value={profiles.active}
                onChange={(e) => onSelectProfile(e.target.value)}
                className={cn(
                  "h-9 rounded-md border border-border bg-surface-2 px-2.5 text-sm text-text",
                  "outline-none transition-colors focus-visible:border-muted/60",
                )}
              >
                {profiles.all.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </label>
          ) : null}

          <Button
            variant={running ? "primary" : "default"}
            onClick={onToggleProxy}
            className={cn(
              "h-10 px-5",
              running &&
                "shadow-[0_0_0_1px_rgba(52,211,153,0.4),0_4px_20px_-4px_rgba(52,211,153,0.5)]",
            )}
          >
            <Power size={16} strokeWidth={2.4} />
            {running ? "Ativo" : "Ativar split"}
          </Button>
        </div>
      </header>

      <div className="flex flex-col gap-4 p-6">
        {/* Saúde / doctor */}
        <Card>
          <CardHeader
            title="Saúde"
            subtitle={
              warnings === 0
                ? "Tudo pronto para interceptar"
                : `${warnings} aviso(s) — revise antes de confiar no split`
            }
            actions={
              <Button size="sm" variant="ghost" onClick={onRerunDoctor}>
                <RefreshCw size={13} />
                Rechecar
              </Button>
            }
          />
          <ul className="divide-y divide-border">
            {doctor.map((check) => (
              <li
                key={check.id}
                className="flex items-start gap-3 px-4 py-2.5 text-sm"
              >
                {check.ok ? (
                  <CheckCircle2
                    size={16}
                    className="mt-0.5 shrink-0 text-accent"
                  />
                ) : (
                  <AlertTriangle
                    size={16}
                    className="mt-0.5 shrink-0 text-warn"
                  />
                )}
                <div className="min-w-0">
                  <span className="text-text">{check.label}</span>
                  {check.hint ? (
                    <p className="mt-0.5 text-xs text-muted">{check.hint}</p>
                  ) : null}
                </div>
                {!check.ok && check.id === "hosts" ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    className="ml-auto"
                    onClick={async () => {
                      await ipc.cleanupHosts();
                      onRerunDoctor();
                    }}
                  >
                    Limpar
                  </Button>
                ) : null}
                <Badge tone={check.ok ? "ok" : "warn"} className="ml-auto">
                  {check.ok ? "ok" : "aviso"}
                </Badge>
              </li>
            ))}
            {doctor.length === 0 ? (
              <li className="px-4 py-3 text-sm text-muted">Sem checagens.</li>
            ) : null}
          </ul>
        </Card>

        {/* Tabela de rotas */}
        <Card>
          <CardHeader
            title="Rotas"
            subtitle={`${routes.length} prefixo(s) — local cai no serviço da sua máquina, passthrough segue para o stage`}
            actions={
              <>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={detectServices}
                  disabled={detecting}
                >
                  <Radar size={14} />
                  {detecting ? "Detectando…" : "Detectar serviços"}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => setAdding((v) => !v)}
                >
                  <Plus size={14} />
                  Adicionar
                </Button>
              </>
            }
          />

          {adding ? (
            <div className="flex items-center gap-2 border-b border-border bg-surface-2/50 px-4 py-3">
              <input
                autoFocus
                value={prefix}
                onChange={(e) => setPrefix(e.target.value)}
                placeholder="/prefixo"
                className="h-8 w-40 rounded-md border border-border bg-bg px-2.5 font-mono text-sm text-text outline-none focus-visible:border-muted/60"
              />
              <span className="text-muted">→</span>
              <input
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                placeholder="localhost:3000"
                onKeyDown={(e) => e.key === "Enter" && submitAdd()}
                className="h-8 w-48 rounded-md border border-border bg-bg px-2.5 font-mono text-sm text-text outline-none focus-visible:border-muted/60"
              />
              <Button size="sm" variant="primary" onClick={submitAdd}>
                Salvar
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setAdding(false)}
              >
                Cancelar
              </Button>
            </div>
          ) : null}

          {detected ? (
            <div className="border-b border-border bg-surface-2/30 px-4 py-3">
              <div className="mb-2 flex items-center gap-2 text-xs text-muted">
                <Radar size={13} />
                {detected.length} serviço(s) local(is) detectado(s)
              </div>
              <ul className="flex flex-col gap-1.5">
                {detected.map((svc) => (
                  <li key={svc.port} className="flex items-center gap-3 text-sm">
                    <code className="font-mono text-text">
                      127.0.0.1:{svc.port}
                    </code>
                    {svc.hint ? (
                      <span className="text-xs text-muted">{svc.hint}</span>
                    ) : null}
                    <Button
                      size="sm"
                      variant="ghost"
                      className="ml-auto"
                      onClick={() => fillFromService(svc)}
                    >
                      <Plus size={13} />
                      Adicionar rota
                    </Button>
                  </li>
                ))}
                {detected.length === 0 ? (
                  <li className="text-xs text-muted">
                    Nenhum serviço local encontrado.
                  </li>
                ) : null}
              </ul>
            </div>
          ) : null}

          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs uppercase tracking-wide text-muted">
                <th className="px-4 py-2 font-medium">Prefixo</th>
                <th className="px-4 py-2 font-medium">Destino</th>
                <th className="px-4 py-2 font-medium">Tipo</th>
                <th className="px-4 py-2 font-medium text-center">Ativa</th>
                <th className="w-10 px-4 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {routes.map((route) => (
                <tr
                  key={route.prefix}
                  className={cn(
                    "transition-colors hover:bg-surface-2/40",
                    !route.enabled && "opacity-55",
                  )}
                >
                  <td className="px-4 py-2 font-mono text-text">
                    {route.prefix}
                  </td>
                  <td className="px-4 py-2 font-mono text-muted">
                    {route.target}
                  </td>
                  <td className="px-4 py-2">
                    <Badge
                      tone={route.kind === "local" ? "local" : "passthrough"}
                    >
                      {route.kind}
                    </Badge>
                  </td>
                  <td className="px-4 py-2">
                    <div className="flex justify-center">
                      <Switch
                        checked={route.enabled}
                        aria-label={`Alternar ${route.prefix}`}
                        disabled={route.kind === "passthrough"}
                        onChange={async (next) => {
                          await ipc.toggleRoute(route.prefix, next);
                          onRoutesChanged();
                        }}
                      />
                    </div>
                  </td>
                  <td className="px-4 py-2 text-right">
                    {route.kind === "passthrough" ? null : (
                      <Button
                        size="sm"
                        variant="danger"
                        aria-label={`Remover ${route.prefix}`}
                        onClick={async () => {
                          await ipc.removeRoute(route.prefix);
                          onRoutesChanged();
                        }}
                      >
                        <Trash2 size={14} />
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
              {routes.length === 0 ? (
                <tr>
                  <td colSpan={5} className="px-4 py-6 text-center text-muted">
                    Nenhuma rota. Adicione um prefixo para começar.
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </Card>
      </div>
    </div>
  );
}
