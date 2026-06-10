import { useEffect, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Layers,
  MousePointerClick,
  Power,
  ToggleRight,
  X,
} from "lucide-react";
import { Button } from "./Button.tsx";
import { SystemFlow } from "./SystemFlow.tsx";
import { cn } from "../lib/cn.ts";

interface Step {
  title: string;
  body: string;
  visual: ReactNode;
}

/** Logo grande (reaproveita o "d" da sidebar). */
function Logo() {
  return (
    <div className="flex flex-col items-center gap-3">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent text-3xl font-bold text-[#04130c]">
        d
      </div>
      <span className="text-lg font-semibold tracking-tight">devsplit</span>
    </div>
  );
}

/** Visual do problema: muitos serviços + memória estourando. */
function ProblemVisual() {
  const services = ["auth", "transporte", "worker", "rabbitmq", "signoz", "telemetria", "+12"];
  return (
    <div className="flex w-full max-w-md flex-col items-center gap-5">
      <div className="flex flex-wrap justify-center gap-2">
        {services.map((s) => (
          <span
            key={s}
            className="rounded-md border border-border bg-surface-2 px-2.5 py-1 text-xs text-muted"
          >
            {s}
          </span>
        ))}
      </div>
      <div className="w-full">
        <div className="mb-1 flex justify-between text-[11px] text-muted">
          <span>Memória do PC</span>
          <span className="text-danger">98%</span>
        </div>
        <div className="h-2.5 w-full overflow-hidden rounded-full bg-surface-2">
          <div className="h-full w-[98%] rounded-full bg-danger/80" />
        </div>
      </div>
    </div>
  );
}

/** Visual de "como usar" — as 3 ações no app. */
function UsageVisual() {
  const items: Array<{ icon: typeof Layers; title: string; desc: string }> = [
    { icon: Layers, title: "Escolha um perfil", desc: "quais serviços rodam local" },
    { icon: ToggleRight, title: "Ligue as rotas", desc: "um switch por caminho" },
    { icon: Power, title: "Ative o split", desc: "uma senha, e pronto" },
  ];
  return (
    <div className="flex w-full max-w-md flex-col gap-2.5">
      {items.map(({ icon: Icon, title, desc }, i) => (
        <div
          key={title}
          className="flex items-center gap-3 rounded-lg border border-border bg-surface-2/60 px-3.5 py-3"
        >
          <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-accent/15 text-accent">
            <Icon size={16} strokeWidth={2.2} />
          </div>
          <div className="min-w-0">
            <div className="text-sm font-medium">{title}</div>
            <div className="text-xs text-muted">{desc}</div>
          </div>
          <span className="ml-auto text-xs tabular-nums text-muted/60">{i + 1}</span>
        </div>
      ))}
    </div>
  );
}

/** Visual final. */
function DoneVisual() {
  return (
    <div className="flex flex-col items-center gap-3">
      <div className="flex h-16 w-16 items-center justify-center rounded-full bg-accent/15 text-accent">
        <Check size={32} strokeWidth={2.5} />
      </div>
      <span className="text-sm text-muted">Tudo pronto pra interceptar.</span>
    </div>
  );
}

const STEPS: Step[] = [
  {
    title: "Bem-vindo ao devsplit",
    body: "Rode localmente só os serviços que você está mexendo. O resto continua no stage — sem subir o ambiente inteiro.",
    visual: <Logo />,
  },
  {
    title: "O problema",
    body: "Subir todos os microsserviços (RabbitMQ, workers, observabilidade…) come RAM demais. PC fraco não aguenta o dev local.",
    visual: <ProblemVisual />,
  },
  {
    title: "Como funciona",
    body: "Seu front continua apontando pro stage. O devsplit intercepta na porta 443 e divide por caminho: os prefixos que você escolher caem no seu localhost; o resto faz passthrough pro stage real, com o certificado validado.",
    visual: <SystemFlow />,
  },
  {
    title: "No app é assim",
    body: "Escolha um perfil, ligue as rotas que quer locais e ative. O devsplit cuida do certificado e do /etc/hosts pra você.",
    visual: <UsageVisual />,
  },
  {
    title: "Bora",
    body: "É só isso. Você pode rever este guia a qualquer momento no botão “?” no topo da janela.",
    visual: <DoneVisual />,
  },
];

export function Onboarding({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [step, setStep] = useState(0);
  const last = STEPS.length - 1;

  // Sempre começa do início ao reabrir.
  useEffect(() => {
    if (open) setStep(0);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowRight") setStep((s) => Math.min(s + 1, last));
      else if (e.key === "ArrowLeft") setStep((s) => Math.max(s - 1, 0));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, last, onClose]);

  if (!open) return null;

  const current = STEPS[step];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-bg/85 p-6"
      style={{ animation: "devsplit-overlay-in 160ms ease-out" }}
    >
      <div
        className="relative flex w-full max-w-2xl flex-col rounded-xl border border-border bg-surface shadow-2xl"
        style={{ animation: "devsplit-card-in 220ms cubic-bezier(0.16,1,0.3,1)" }}
      >
        {/* topo */}
        <div className="flex items-center justify-between px-5 pt-4">
          <span className="text-[11px] uppercase tracking-widest text-muted">
            Passo {step + 1} de {STEPS.length}
          </span>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted transition-colors hover:bg-surface-2 hover:text-text"
            aria-label="Pular onboarding"
          >
            <X size={16} />
          </button>
        </div>

        {/* visual (área herói) */}
        <div className="mx-5 mt-3 flex h-64 items-center justify-center rounded-lg border border-border bg-bg/40 p-4">
          <div key={step} style={{ animation: "devsplit-step-in 260ms ease-out" }} className="flex h-full w-full items-center justify-center">
            {current.visual}
          </div>
        </div>

        {/* texto */}
        <div key={`txt-${step}`} className="px-6 pt-5" style={{ animation: "devsplit-step-in 260ms ease-out 40ms both" }}>
          <h2 className="text-lg font-semibold tracking-tight">{current.title}</h2>
          <p className="mt-1.5 text-sm leading-relaxed text-muted">{current.body}</p>
        </div>

        {/* rodapé */}
        <div className="flex items-center justify-between px-6 py-5">
          {/* dots */}
          <div className="flex items-center gap-1.5">
            {STEPS.map((_, i) => (
              <button
                key={i}
                onClick={() => setStep(i)}
                aria-label={`Ir para o passo ${i + 1}`}
                className={cn(
                  "h-1.5 rounded-full transition-all duration-200",
                  i === step ? "w-5 bg-accent" : "w-1.5 bg-border hover:bg-muted",
                )}
              />
            ))}
          </div>

          <div className="flex items-center gap-2">
            {step > 0 && (
              <Button variant="ghost" size="sm" onClick={() => setStep((s) => s - 1)}>
                <ArrowLeft size={14} />
                Voltar
              </Button>
            )}
            {step < last ? (
              <Button variant="primary" size="sm" onClick={() => setStep((s) => s + 1)}>
                Próximo
                <ArrowRight size={14} />
              </Button>
            ) : (
              <Button variant="primary" size="sm" onClick={onClose}>
                <MousePointerClick size={14} />
                Começar
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
