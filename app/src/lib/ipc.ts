// IPC layer for devsplit.
//
// In the Tauri webview (`__TAURI_INTERNALS__` present) every call routes to a
// real `#[tauri::command]` via `invoke`, and events arrive over the Tauri event
// bus. In a plain browser (`npm run dev`/`preview`, CI build) we fall back to a
// self-contained MOCK so the UI is fully explorable headless.

import { encodeJwt } from "./jwt.ts";

export type RouteKind = "local" | "passthrough";

export interface Status {
  running: boolean;
  interceptHost: string;
  listenAddr: string;
  hosts: string[];
}

export interface Route {
  prefix: string;
  target: string;
  kind: RouteKind;
  enabled: boolean;
}

export interface DoctorCheck {
  id: string;
  label: string;
  ok: boolean;
  hint?: string;
}

export interface Profiles {
  active: string;
  all: string[];
}

export type Decision = "local" | "passthrough";

export interface TrafficEntry {
  id: number;
  ts: number;
  method: string;
  host: string;
  path: string;
  decision: Decision;
  status?: number;
  latencyMs?: number;
  reqSize?: number;
  respSize?: number;
}

export interface RequestDetail {
  id: number;
  ts: number;
  method: string;
  host: string;
  path: string;
  decision: Decision;
  status?: number;
  latencyMs?: number;
  reqHeaders: Array<[string, string]>;
  reqBody?: string;
  reqBodyTruncated: boolean;
  reqSize?: number;
  respHeaders: Array<[string, string]>;
  respBody?: string;
  respBodyTruncated: boolean;
  respSize?: number;
  redacted: boolean;
}

export interface LocalService {
  port: number;
  hint?: string;
}

export type NoticeLevel = "info" | "warn" | "error";

export interface Notice {
  level: NoticeLevel;
  message: string;
}

export type Unlisten = () => void;

export interface DevsplitIpc {
  getStatus(): Promise<Status>;
  startProxy(): Promise<void>;
  stopProxy(): Promise<void>;
  listRoutes(): Promise<Route[]>;
  addRoute(prefix: string, target: string): Promise<void>;
  removeRoute(prefix: string): Promise<void>;
  toggleRoute(prefix: string, enabled: boolean): Promise<void>;
  runDoctor(): Promise<DoctorCheck[]>;
  getProfiles(): Promise<Profiles>;
  setProfile(name: string): Promise<void>;
  installCert(): Promise<string>;
  reresolveUpstream(): Promise<Status>;
  getRequestDetail(id: number): Promise<RequestDetail>;
  detectServices(): Promise<LocalService[]>;
  cleanupHosts(): Promise<void>;
  onTraffic(cb: (e: TrafficEntry) => void): Promise<Unlisten>;
  onStatus(cb: (s: Status) => void): Promise<Unlisten>;
  onNotice(cb: (n: Notice) => void): Promise<Unlisten>;
}

const isTauri = "__TAURI_INTERNALS__" in window;

// --- Real Tauri-backed implementation -------------------------------------

function createTauriIpc(): DevsplitIpc {
  // Dynamic import so the bundle never hard-requires Tauri at module load.
  const core = import("@tauri-apps/api/core");
  const event = import("@tauri-apps/api/event");

  return {
    async getStatus() {
      return (await core).invoke<Status>("get_status");
    },
    async startProxy() {
      await (await core).invoke("start_proxy");
    },
    async stopProxy() {
      await (await core).invoke("stop_proxy");
    },
    async listRoutes() {
      return (await core).invoke<Route[]>("list_routes");
    },
    async addRoute(prefix, target) {
      await (await core).invoke("add_route", { prefix, target });
    },
    async removeRoute(prefix) {
      await (await core).invoke("remove_route", { prefix });
    },
    async toggleRoute(prefix, enabled) {
      await (await core).invoke("toggle_route", { prefix, enabled });
    },
    async runDoctor() {
      return (await core).invoke<DoctorCheck[]>("run_doctor");
    },
    async getProfiles() {
      return (await core).invoke<Profiles>("get_profiles");
    },
    async setProfile(name) {
      await (await core).invoke("set_profile", { name });
    },
    async installCert() {
      return (await core).invoke<string>("install_cert");
    },
    async reresolveUpstream() {
      return (await core).invoke<Status>("reresolve_upstream");
    },
    async getRequestDetail(id) {
      return (await core).invoke<RequestDetail>("get_request_detail", { id });
    },
    async detectServices() {
      return (await core).invoke<LocalService[]>("detect_local_services");
    },
    async cleanupHosts() {
      await (await core).invoke("cleanup_hosts");
    },
    async onTraffic(cb) {
      const un = await (await event).listen<TrafficEntry>(
        "proxy://traffic",
        (e) => cb(e.payload),
      );
      return un;
    },
    async onStatus(cb) {
      const un = await (await event).listen<Status>("proxy://status", (e) =>
        cb(e.payload),
      );
      return un;
    },
    async onNotice(cb) {
      const un = await (await event).listen<Notice>("proxy://notice", (e) =>
        cb(e.payload),
      );
      return un;
    },
  };
}

// --- Mock implementation (headless browser) --------------------------------

const MOCK_HOST = "api.stage.acme.dev";

function mintToken(ttlSeconds: number): string {
  const iat = Math.floor(Date.now() / 1000);
  return encodeJwt(
    { alg: "HS256", typ: "JWT", kid: "stage-2024" },
    {
      iss: "https://auth.stage.acme.dev",
      aud: "devsplit-stage",
      sub: "usr_8F3KQ2",
      name: "dev@acme.dev",
      roles: ["motorista", "admin-transporte"],
      scope: "transporte:read transporte:write financeiro:read",
      iat,
      exp: iat + ttlSeconds,
    },
  );
}

function pickMockStatus(path: string): number {
  const r = Math.random();
  if (path === "/auth/login") return r < 0.18 ? 401 : 200;
  if (r < 0.76) return 200;
  if (r < 0.84) return 201;
  if (r < 0.9) return 401;
  if (r < 0.95) return 404;
  return 502;
}

function buildMockReqBody(path: string, prefix: string, method: string): unknown {
  if (method !== "POST" && method !== "PUT" && method !== "PATCH") return undefined;
  if (path === "/auth/login") return { email: "dev@acme.dev", password: "••••••" };
  if (path === "/auth/refresh") return { refreshToken: "rt_8d3f…a91" };
  if (prefix === "/transporte") return { cte: { numero: 91823, serie: 1 }, emitir: true };
  if (prefix === "/financeiro") return { fatura: "F-2024-0917", valor: 1899.9, moeda: "BRL" };
  return { ping: true };
}

function buildMockRespBody(path: string, prefix: string, status: number): unknown {
  if (status >= 500) return { error: "bad_gateway", message: "upstream de stage indisponível" };
  if (status === 404) return { error: "not_found", path };
  if (status === 401) return { error: "unauthorized", message: "token ausente ou expirado" };
  if (path === "/auth/login" || path === "/auth/refresh") {
    return { accessToken: "<jwt rotacionado>", tokenType: "Bearer", expiresIn: 420 };
  }
  if (prefix === "/transporte") return { ok: true, protocolo: "9f2a4c", itens: 3 };
  if (prefix === "/financeiro") return { ok: true, saldo: 10432.55, pendentes: 2 };
  return { ok: true };
}

function createMockIpc(): DevsplitIpc {
  let running = true;
  let activeProfile = "transporte";
  let nextId = 1;

  let routes: Route[] = [
    {
      prefix: "/transporte",
      target: "localhost:3000",
      kind: "local",
      enabled: true,
    },
    { prefix: "/auth", target: "localhost:3001", kind: "local", enabled: true },
    {
      prefix: "/financeiro",
      target: "localhost:3002",
      kind: "local",
      enabled: false,
    },
    {
      prefix: "/*",
      target: MOCK_HOST,
      kind: "passthrough",
      enabled: true,
    },
  ];

  const statusListeners = new Set<(s: Status) => void>();
  const trafficListeners = new Set<(e: TrafficEntry) => void>();
  const noticeListeners = new Set<(n: Notice) => void>();

  // Bearer the synthetic API currently hands out; re-minted on auth calls so
  // the Session view sees a live, ticking token.
  let currentToken = mintToken(420);

  interface MockRecord {
    entry: TrafficEntry;
    token?: string;
    reqBody?: string;
    respBody: string;
  }
  const store = new Map<number, MockRecord>();

  const delay = (ms: number): Promise<void> => {
    const { promise, resolve } = Promise.withResolvers<void>();
    setTimeout(resolve, ms);
    return promise;
  };

  const snapshotStatus = (): Status => ({
    running,
    interceptHost: MOCK_HOST,
    listenAddr: "127.0.0.1:8443",
    hosts: [MOCK_HOST, "auth.stage.acme.dev"],
  });

  const emitStatus = () => {
    const s = snapshotStatus();
    for (const cb of statusListeners) cb(s);
  };

  const emitNotice = (level: NoticeLevel, message: string) => {
    for (const cb of noticeListeners) cb({ level, message });
  };

  // Authenticated-looking endpoints across every prefix class.
  const SAMPLE_PATHS: Array<{
    path: string;
    prefix: string;
    method: string;
    auth: boolean;
  }> = [
    { path: "/transporte/cte/emitir", prefix: "/transporte", method: "POST", auth: true },
    { path: "/transporte/manifesto/123", prefix: "/transporte", method: "GET", auth: true },
    { path: "/transporte/veiculos", prefix: "/transporte", method: "GET", auth: true },
    { path: "/auth/login", prefix: "/auth", method: "POST", auth: false },
    { path: "/auth/refresh", prefix: "/auth", method: "POST", auth: true },
    { path: "/financeiro/faturas", prefix: "/financeiro", method: "GET", auth: true },
    { path: "/financeiro/pagamentos/9f2", prefix: "/financeiro", method: "POST", auth: true },
    { path: "/outra/catalogo/itens", prefix: "/*", method: "GET", auth: false },
    { path: "/outra/relatorios/diario", prefix: "/*", method: "GET", auth: true },
    { path: "/health", prefix: "/*", method: "GET", auth: false },
  ];

  setInterval(() => {
    if (!running || trafficListeners.size === 0) return;
    const sample = SAMPLE_PATHS[Math.floor(Math.random() * SAMPLE_PATHS.length)];
    const route = routes.find((r) => r.prefix === sample.prefix);
    const decision: Decision =
      route && route.kind === "local" && route.enabled
        ? "local"
        : "passthrough";
    const status = pickMockStatus(sample.path);
    const latencyMs = Math.min(
      400,
      Math.max(
        10,
        Math.round(
          decision === "local"
            ? 10 + Math.random() * 55
            : 60 + Math.random() * 340,
        ),
      ),
    );
    const reqBodyObj = buildMockReqBody(sample.path, sample.prefix, sample.method);
    const respBodyObj = buildMockRespBody(sample.path, sample.prefix, status);
    const reqBody = reqBodyObj ? JSON.stringify(reqBodyObj, null, 2) : undefined;
    const respBody = JSON.stringify(respBodyObj, null, 2);
    const id = nextId++;
    const entry: TrafficEntry = {
      id,
      ts: Date.now(),
      method: sample.method,
      host: MOCK_HOST,
      path: sample.path,
      decision,
      status,
      latencyMs,
      reqSize: reqBody ? new TextEncoder().encode(reqBody).length : 0,
      respSize: new TextEncoder().encode(respBody).length,
    };
    store.set(id, {
      entry,
      token: sample.auth ? currentToken : undefined,
      reqBody,
      respBody,
    });
    if (store.size > 500) {
      const oldest = store.keys().next().value;
      if (oldest !== undefined) store.delete(oldest);
    }
    if (
      (sample.path === "/auth/login" || sample.path === "/auth/refresh") &&
      status < 400
    ) {
      currentToken = mintToken(420);
    }
    for (const cb of trafficListeners) cb(entry);
    if (status >= 500 && Math.random() < 0.6) {
      emitNotice("error", `502 em ${sample.path} — gateway de stage instável`);
    }
  }, 900);

  setTimeout(() => emitNotice("info", "Modo mock — tráfego sintético ativo"), 800);

  return {
    async getStatus() {
      return snapshotStatus();
    },
    async startProxy() {
      running = true;
      emitStatus();
      emitNotice("info", `Split ligado — interceptando ${MOCK_HOST}`);
    },
    async stopProxy() {
      running = false;
      emitStatus();
      emitNotice("warn", "Split desligado");
    },
    async listRoutes() {
      return routes.map((r) => ({ ...r }));
    },
    async addRoute(prefix, target) {
      if (routes.some((r) => r.prefix === prefix)) return;
      const next: Route = { prefix, target, kind: "local", enabled: true };
      const idx = routes.findIndex((r) => r.prefix === "/*");
      if (idx === -1) routes = [...routes, next];
      else routes = [...routes.slice(0, idx), next, ...routes.slice(idx)];
    },
    async removeRoute(prefix) {
      routes = routes.filter((r) => r.prefix !== prefix);
    },
    async toggleRoute(prefix, enabled) {
      routes = routes.map((r) => (r.prefix === prefix ? { ...r, enabled } : r));
    },
    async runDoctor() {
      return [
        {
          id: "cert",
          label: "Certificado raiz confiável (mkcert)",
          ok: true,
        },
        { id: "hosts", label: `Entrada hosts → ${MOCK_HOST}`, ok: true },
        { id: "port", label: "Porta 8443 disponível", ok: true },
        { id: "upstream", label: "Gateway de stage alcançável", ok: true },
        {
          id: "autostart",
          label: "Iniciar com o sistema",
          ok: false,
          hint: "Autostart desativado — ative em Config para subir o proxy no login.",
        },
      ];
    },
    async getProfiles() {
      return {
        active: activeProfile,
        all: ["transporte", "tudo-local", "qa", "stage"],
      };
    },
    async setProfile(name) {
      activeProfile = name;
    },
    async installCert() {
      await delay(400);
      emitNotice("info", "Certificado raiz instalado (mock)");
      return "CA instalada no NSS (mock)";
    },
    async reresolveUpstream() {
      return snapshotStatus();
    },
    async getRequestDetail(id) {
      const rec = store.get(id);
      if (!rec) {
        return {
          id,
          ts: Date.now(),
          method: "GET",
          host: MOCK_HOST,
          path: "/",
          decision: "passthrough",
          status: 200,
          reqHeaders: [["host", MOCK_HOST]],
          reqBodyTruncated: false,
          respHeaders: [["content-type", "application/json"]],
          respBody: "{}",
          respBodyTruncated: false,
          redacted: true,
        };
      }
      const e = rec.entry;
      const reqHeaders: Array<[string, string]> = [
        ["host", e.host],
        ["user-agent", "devsplit/0.1 (mock)"],
        ["accept", "application/json"],
        ["x-request-id", `req_${e.id}`],
      ];
      if (rec.token) reqHeaders.push(["authorization", `Bearer ${rec.token}`]);
      reqHeaders.push(["cookie", "session=••••••••"]);
      if (rec.reqBody) reqHeaders.push(["content-type", "application/json"]);
      const local = routes.find(
        (r) => r.kind === "local" && e.path.startsWith(r.prefix),
      );
      const servedBy =
        e.decision === "local" && local ? local.target : MOCK_HOST;
      const respHeaders: Array<[string, string]> = [
        ["content-type", "application/json; charset=utf-8"],
        ["server", e.decision === "local" ? "devsplit-local" : "stage-gateway/1.4"],
        ["x-served-by", servedBy],
        ["x-devsplit-decision", e.decision],
        ["content-length", String(e.respSize ?? 0)],
      ];
      return {
        id: e.id,
        ts: e.ts,
        method: e.method,
        host: e.host,
        path: e.path,
        decision: e.decision,
        status: e.status,
        latencyMs: e.latencyMs,
        reqHeaders,
        reqBody: rec.reqBody,
        reqBodyTruncated: false,
        reqSize: e.reqSize,
        respHeaders,
        respBody: rec.respBody,
        respBodyTruncated: false,
        respSize: e.respSize,
        redacted: true,
      };
    },
    async detectServices() {
      await delay(280);
      return [
        { port: 3000, hint: "Vite (transporte-web)" },
        { port: 3001, hint: "auth-service (node)" },
        { port: 3002, hint: "financeiro-api" },
        { port: 5432, hint: "PostgreSQL" },
        { port: 8080 },
      ];
    },
    async cleanupHosts() {
      // mock: nao ha bloco real de /etc/hosts p/ limpar
    },
    async onTraffic(cb) {
      trafficListeners.add(cb);
      return () => {
        trafficListeners.delete(cb);
      };
    },
    async onStatus(cb) {
      statusListeners.add(cb);
      return () => {
        statusListeners.delete(cb);
      };
    },
    async onNotice(cb) {
      noticeListeners.add(cb);
      return () => {
        noticeListeners.delete(cb);
      };
    },
  };
}

export const ipc: DevsplitIpc = isTauri ? createTauriIpc() : createMockIpc();
export const RUNTIME: "tauri" | "mock" = isTauri ? "tauri" : "mock";
