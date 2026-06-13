import { useCallback, useEffect, useState } from "react";
import {
  Activity,
  Route as RouteIcon,
  ShieldCheck,
  Server,
  Settings,
  HelpCircle,
  KeyRound,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  ipc,
  RUNTIME,
  type DoctorCheck,
  type Profiles,
  type Route,
  type Status,
} from "./lib/ipc.ts";
import { cn } from "./lib/cn.ts";
import { StatusDot } from "./components/StatusDot.tsx";
import { RoutesView } from "./views/RoutesView.tsx";
import { TrafficView } from "./views/TrafficView.tsx";
import { CertificateView } from "./views/CertificateView.tsx";
import { HostsView } from "./views/HostsView.tsx";
import { ConfigView } from "./views/ConfigView.tsx";
import { Onboarding } from "./components/Onboarding.tsx";
import { SessionView } from "./views/SessionView.tsx";
import { CommandPalette } from "./components/CommandPalette.tsx";
import { Toaster } from "./components/Toast.tsx";

export type ViewId = "routes" | "traffic" | "session" | "cert" | "hosts" | "config";

const NAV: Array<{ id: ViewId; label: string; icon: LucideIcon }> = [
  { id: "routes", label: "Rotas", icon: RouteIcon },
  { id: "traffic", label: "Tráfego", icon: Activity },
  { id: "session", label: "Sessão", icon: KeyRound },
  { id: "cert", label: "Certificado", icon: ShieldCheck },
  { id: "hosts", label: "Hosts", icon: Server },
  { id: "config", label: "Config", icon: Settings },
];

export default function App() {
  const [view, setView] = useState<ViewId>("routes");
  const [status, setStatus] = useState<Status | null>(null);
  const [routes, setRoutes] = useState<Route[]>([]);
  const [doctor, setDoctor] = useState<DoctorCheck[]>([]);
  const [profiles, setProfiles] = useState<Profiles | null>(null);
  const [showOnboarding, setShowOnboarding] = useState(
    () => localStorage.getItem("devsplit:onboarded") !== "1",
  );
  const [paletteOpen, setPaletteOpen] = useState(false);
  const closeOnboarding = useCallback(() => {
    localStorage.setItem("devsplit:onboarded", "1");
    setShowOnboarding(false);
  }, []);

  const refreshRoutes = useCallback(async () => {
    setRoutes(await ipc.listRoutes());
  }, []);

  const refreshDoctor = useCallback(async () => {
    setDoctor(await ipc.runDoctor());
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      setStatus(await ipc.getStatus());
      setProfiles(await ipc.getProfiles());
      await refreshRoutes();
      await refreshDoctor();
      unlisten = await ipc.onStatus(setStatus);
    })();
    return () => unlisten?.();
  }, [refreshRoutes, refreshDoctor]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toggleProxy = useCallback(async () => {
    if (!status) return;
    try {
      if (status.running) await ipc.stopProxy();
      else await ipc.startProxy();
    } catch {
      // o backend ja emite proxy://notice (toast) na falha; aqui so evitamos a
      // promise rejeitada solta que deixava o clique "sem reacao".
    }
    setStatus(await ipc.getStatus());
    // split liga/desliga a CA no navegador -> reflete na aba Certificado.
    await refreshDoctor();
  }, [status, refreshDoctor]);

  const selectProfile = useCallback(async (name: string) => {
    await ipc.setProfile(name);
    setProfiles(await ipc.getProfiles());
    setRoutes(await ipc.listRoutes());
  }, []);

  return (
    <>
    <div className="flex h-screen w-screen overflow-hidden bg-bg text-text">
      <aside className="flex w-52 shrink-0 flex-col border-r border-border bg-surface">
        <div className="flex items-center gap-2 px-4 py-4">
          <div className="flex h-6 w-6 items-center justify-center rounded-md bg-accent text-[13px] font-bold text-[#04130c]">
            d
          </div>
          <span className="text-sm font-semibold tracking-tight">devsplit</span>
          <span className="ml-auto text-[10px] uppercase tracking-widest text-muted">
            {RUNTIME}
          </span>
          <button
            onClick={() => setShowOnboarding(true)}
            className="rounded-md p-1 text-muted transition-colors hover:bg-surface-2 hover:text-text"
            aria-label="Como funciona o devsplit"
            title="Como funciona"
          >
            <HelpCircle size={15} />
          </button>
        </div>

        <nav className="flex flex-col gap-0.5 px-2">
          {NAV.map((item) => {
            const Icon = item.icon;
            const active = view === item.id;
            return (
              <button
                key={item.id}
                onClick={() => setView(item.id)}
                className={cn(
                  "flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm",
                  "transition-colors duration-150 outline-none",
                  active
                    ? "bg-surface-2 text-text"
                    : "text-muted hover:text-text hover:bg-surface-2/60",
                )}
              >
                <Icon size={16} strokeWidth={2} />
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="mt-auto flex items-center gap-2 border-t border-border px-4 py-3 text-xs text-muted">
          <StatusDot tone={status?.running ? "on" : "off"} pulse />
          <span>{status?.running ? "Interceptando" : "Parado"}</span>
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        {view === "routes" && (
          <RoutesView
            status={status}
            routes={routes}
            doctor={doctor}
            profiles={profiles}
            onToggleProxy={toggleProxy}
            onSelectProfile={selectProfile}
            onRoutesChanged={refreshRoutes}
            onRerunDoctor={refreshDoctor}
          />
        )}
        {view === "traffic" && <TrafficView />}
        {view === "session" && <SessionView />}
        {view === "cert" && (
          <CertificateView doctor={doctor} onRecheck={refreshDoctor} />
        )}
        {view === "hosts" && <HostsView status={status} routes={routes} />}
        {view === "config" && (
          <ConfigView status={status} profiles={profiles} />
        )}
      </main>
    </div>
      <Onboarding open={showOnboarding} onClose={closeOnboarding} />
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        status={status}
        routes={routes}
        profiles={profiles}
        nav={NAV}
        onNavigate={setView}
        onToggleProxy={toggleProxy}
        onSelectProfile={selectProfile}
        onRefresh={refreshRoutes}
      />
      <Toaster />
    </>
  );
}
