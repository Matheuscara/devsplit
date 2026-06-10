import { RUNTIME, type Profiles, type Status } from "../lib/ipc.ts";
import { Card, CardHeader } from "../components/Card.tsx";
import { Badge } from "../components/Badge.tsx";
import { StatusDot } from "../components/StatusDot.tsx";

interface ConfigViewProps {
  status: Status | null;
  profiles: Profiles | null;
}

export function ConfigView({ status, profiles }: ConfigViewProps) {
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <header className="border-b border-border px-6 py-4">
        <h1 className="text-lg font-semibold text-text">Config</h1>
        <p className="mt-0.5 text-xs text-muted">
          Estado atual do devsplit e do ambiente de execução
        </p>
      </header>

      <div className="flex flex-col gap-4 p-6">
        <Card>
          <CardHeader title="Runtime" />
          <dl className="divide-y divide-border text-sm">
            <div className="flex items-center justify-between px-4 py-2.5">
              <dt className="text-muted">Backend</dt>
              <dd className="flex items-center gap-2">
                <Badge tone={RUNTIME === "tauri" ? "ok" : "warn"}>
                  {RUNTIME === "tauri" ? "Tauri (nativo)" : "Mock (navegador)"}
                </Badge>
              </dd>
            </div>
            <div className="flex items-center justify-between px-4 py-2.5">
              <dt className="text-muted">Proxy</dt>
              <dd className="flex items-center gap-2 text-text">
                <StatusDot tone={status?.running ? "on" : "off"} />
                {status?.running ? "rodando" : "parado"}
              </dd>
            </div>
            <div className="flex items-center justify-between px-4 py-2.5">
              <dt className="text-muted">Endereço de escuta</dt>
              <dd className="font-mono text-text">
                {status?.listenAddr ?? "—"}
              </dd>
            </div>
            <div className="flex items-center justify-between px-4 py-2.5">
              <dt className="text-muted">Domínio interceptado</dt>
              <dd className="font-mono text-text">
                {status?.interceptHost ?? "—"}
              </dd>
            </div>
          </dl>
        </Card>

        <Card>
          <CardHeader title="Perfis" subtitle="Conjuntos de rotas alternáveis" />
          <ul className="divide-y divide-border text-sm">
            {(profiles?.all ?? []).map((p) => (
              <li
                key={p}
                className="flex items-center justify-between px-4 py-2.5"
              >
                <span className="text-text">{p}</span>
                {p === profiles?.active ? (
                  <Badge tone="ok">ativo</Badge>
                ) : (
                  <span className="text-xs text-muted">inativo</span>
                )}
              </li>
            ))}
            {!profiles || profiles.all.length === 0 ? (
              <li className="px-4 py-3 text-muted">Sem perfis.</li>
            ) : null}
          </ul>
        </Card>
      </div>
    </div>
  );
}
