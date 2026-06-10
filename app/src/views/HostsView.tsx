import { Server } from "lucide-react";
import type { Route, Status } from "../lib/ipc.ts";
import { Card, CardHeader } from "../components/Card.tsx";
import { Badge } from "../components/Badge.tsx";

interface HostsViewProps {
  status: Status | null;
  routes: Route[];
}

export function HostsView({ status, routes }: HostsViewProps) {
  const host = status?.interceptHost ?? "—";
  const localCount = routes.filter(
    (r) => r.kind === "local" && r.enabled,
  ).length;

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <header className="border-b border-border px-6 py-4">
        <h1 className="text-lg font-semibold text-text">Hosts</h1>
        <p className="mt-0.5 text-xs text-muted">
          Entrada em /etc/hosts que aponta o domínio do stage para o proxy local
        </p>
      </header>

      <div className="flex flex-col gap-4 p-6">
        <Card>
          <CardHeader
            title="Mapeamento ativo"
            actions={
              <Badge tone={status?.running ? "ok" : "neutral"}>
                {status?.running ? "aplicado" : "inativo"}
              </Badge>
            }
          />
          <div className="flex items-center gap-3 px-4 py-4">
            <Server size={24} className="text-muted" />
            <code className="font-mono text-sm text-text">
              127.0.0.1
              <span className="px-3 text-muted">→</span>
              {host}
            </code>
          </div>
        </Card>

        <Card>
          <CardHeader
            title="Resumo do split"
            subtitle={`${localCount} prefixo(s) caindo na sua máquina; o restante segue para o stage`}
          />
          <ul className="divide-y divide-border text-sm">
            {routes.map((r) => (
              <li
                key={r.prefix}
                className="flex items-center gap-3 px-4 py-2.5"
              >
                <code className="font-mono text-text">{host}{r.prefix}</code>
                <span className="text-muted">→</span>
                <code className="font-mono text-muted">{r.target}</code>
                <Badge
                  tone={r.kind === "local" ? "local" : "passthrough"}
                  className="ml-auto"
                >
                  {r.kind}
                </Badge>
              </li>
            ))}
          </ul>
        </Card>
      </div>
    </div>
  );
}
