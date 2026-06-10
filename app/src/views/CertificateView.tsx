import { useState } from "react";
import {
  ShieldCheck,
  AlertTriangle,
  CheckCircle2,
  ShieldPlus,
  RefreshCw,
} from "lucide-react";
import { ipc, type DoctorCheck } from "../lib/ipc.ts";
import { Card, CardHeader } from "../components/Card.tsx";
import { Badge } from "../components/Badge.tsx";
import { Button } from "../components/Button.tsx";

interface CertificateViewProps {
  doctor: DoctorCheck[];
  /** Optional: re-run doctor after a successful CA install. */
  onRecheck?: () => void | Promise<void>;
}

export function CertificateView({ doctor, onRecheck }: CertificateViewProps) {
  const cert = doctor.find((c) => c.id === "cert");
  const trusted = cert?.ok ?? false;

  const [installing, setInstalling] = useState(false);
  const [installOk, setInstallOk] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  const [reresolving, setReresolving] = useState(false);
  const [reresolveMsg, setReresolveMsg] = useState<string | null>(null);

  async function handleInstall() {
    setInstalling(true);
    setInstallError(null);
    setInstallOk(false);
    try {
      await ipc.installCert();
      setInstallOk(true);
      await onRecheck?.();
    } catch (e) {
      setInstallError(e instanceof Error ? e.message : String(e));
    } finally {
      setInstalling(false);
    }
  }

  async function handleReresolve() {
    setReresolving(true);
    setReresolveMsg(null);
    try {
      const s = await ipc.reresolveUpstream();
      setReresolveMsg(`IP do stage atualizado — ${s.interceptHost}`);
    } catch (e) {
      setReresolveMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setReresolving(false);
    }
  }

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <header className="border-b border-border px-6 py-4">
        <h1 className="text-lg font-semibold text-text">Certificado</h1>
        <p className="mt-0.5 text-xs text-muted">
          Raiz local (mkcert) que torna o HTTPS interceptado confiável
        </p>
      </header>

      <div className="flex flex-col gap-4 p-6">
        <Card>
          <CardHeader title="Status da CA" />
          <div className="flex items-center gap-3 px-4 py-4">
            <ShieldCheck
              size={28}
              className={trusted ? "text-accent" : "text-warn"}
            />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-text">
                  {trusted ? "CA instalada e confiável" : "CA não confiável"}
                </span>
                <Badge tone={trusted ? "ok" : "warn"}>
                  {trusted ? "ok" : "ação necessária"}
                </Badge>
              </div>
              <p className="mt-0.5 text-xs text-muted">
                {cert?.hint ??
                  "A raiz mkcert assina o certificado servido para o domínio interceptado."}
              </p>
            </div>
            <Button
              variant="primary"
              onClick={handleInstall}
              disabled={installing}
            >
              <ShieldPlus size={16} />
              {installing ? "Instalando..." : "Instalar certificado"}
            </Button>
          </div>
        </Card>

        <Card>
          <CardHeader title="Detalhes" />
          <dl className="divide-y divide-border text-sm">
            {[
              { k: "Ferramenta", v: "mkcert (bundlado)" },
              { k: "Algoritmo", v: "ECDSA P-256" },
              { k: "Validade", v: "825 dias" },
              {
                k: "Trust store",
                v: trusted ? "system + nss" : "pendente",
              },
            ].map((row) => (
              <div
                key={row.k}
                className="flex items-center justify-between px-4 py-2.5"
              >
                <dt className="text-muted">{row.k}</dt>
                <dd className="font-mono text-text">{row.v}</dd>
              </div>
            ))}
          </dl>
          <div className="flex items-center justify-between gap-3 border-t border-border px-4 py-3">
            <span className="text-xs text-muted">
              Re-resolve o IP real do gateway de stage.
            </span>
            <Button
              variant="default"
              size="sm"
              onClick={handleReresolve}
              disabled={reresolving}
            >
              <RefreshCw
                size={14}
                className={reresolving ? "animate-spin" : undefined}
              />
              {reresolving ? "Re-resolvendo..." : "Re-resolver IP do stage"}
            </Button>
          </div>
        </Card>

        {installError && (
          <div className="flex items-start gap-2 rounded-md border border-danger/25 bg-danger/10 px-4 py-3 text-sm text-danger">
            <AlertTriangle size={16} className="mt-0.5 shrink-0" />
            <span>{installError}</span>
          </div>
        )}

        {installOk && (
          <div className="flex items-start gap-2 rounded-md border border-accent/25 bg-accent/10 px-4 py-3 text-sm text-accent">
            <CheckCircle2 size={16} className="mt-0.5 shrink-0" />
            <span>Certificado instalado no navegador</span>
          </div>
        )}

        {reresolveMsg && (
          <div className="flex items-start gap-2 rounded-md border border-accent/25 bg-accent/10 px-4 py-3 text-sm text-accent">
            <RefreshCw size={16} className="mt-0.5 shrink-0" />
            <span>{reresolveMsg}</span>
          </div>
        )}

        {!trusted ? (
          <div className="flex items-start gap-2 rounded-md border border-warn/25 bg-warn/10 px-4 py-3 text-sm text-warn">
            <AlertTriangle size={16} className="mt-0.5 shrink-0" />
            <span>
              Reinstale a CA pelo bootstrap para que o navegador pare de avisar
              sobre certificado inválido.
            </span>
          </div>
        ) : (
          <div className="flex items-start gap-2 rounded-md border border-accent/25 bg-accent/10 px-4 py-3 text-sm text-accent">
            <CheckCircle2 size={16} className="mt-0.5 shrink-0" />
            <span>Nada a fazer — o HTTPS interceptado já é confiável.</span>
          </div>
        )}
      </div>
    </div>
  );
}
