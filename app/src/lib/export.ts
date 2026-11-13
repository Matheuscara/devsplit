// curl + HAR serializers for a single captured request, plus a file download.

import { RUNTIME, type RequestDetail } from "./ipc.ts";

const REDACTED = "<redacted>";

// Framing/hop-by-hop headers conflict with curl's own body/encoding handling.
const SKIP_HEADERS: Record<string, true> = {
  "content-length": true,
  "transfer-encoding": true,
  connection: true,
  "accept-encoding": true,
};

function exportableHeaders(headers: Array<[string, string]>): Array<[string, string]> {
  return headers.filter(
    ([k, v]) => v !== REDACTED && !SKIP_HEADERS[k.toLowerCase()],
  );
}

function headerValue(headers: Array<[string, string]>, name: string): string | undefined {
  const lower = name.toLowerCase();
  for (const [k, v] of headers) {
    if (k.toLowerCase() === lower) return v;
  }
  return undefined;
}

function fullUrl(d: RequestDetail): string {
  return `https://${d.host}${d.path}${d.query ? `?${d.query}` : ""}`;
}

const shQuote = (s: string) => `'${s.replace(/'/g, "'\\''")}'`;

export function toCurl(d: RequestDetail): string {
  const lines = [`curl -X ${d.method} ${shQuote(fullUrl(d))}`];
  for (const [k, v] of exportableHeaders(d.reqHeaders)) {
    lines.push(`  -H ${shQuote(`${k}: ${v}`)}`);
  }
  if (d.reqBody) {
    lines.push(`  --data-raw ${shQuote(d.reqBody)}`);
  }
  return lines.join(" \\\n");
}

interface HarNameValue {
  name: string;
  value: string;
}

function harQuery(query: string | null | undefined): HarNameValue[] {
  if (!query) return [];
  return [...new URLSearchParams(query)].map(([name, value]) => ({ name, value }));
}

export function toHar(d: RequestDetail): unknown {
  const reqHeaders: HarNameValue[] = exportableHeaders(d.reqHeaders).map(
    ([name, value]) => ({ name, value }),
  );
  const respHeaders: HarNameValue[] = d.respHeaders
    .filter(([, v]) => v !== REDACTED)
    .map(([name, value]) => ({ name, value }));
  const reqMime = headerValue(d.reqHeaders, "content-type") ?? "application/json";
  const respMime = headerValue(d.respHeaders, "content-type") ?? "application/json";
  const wait = d.latencyMs ?? 0;

  return {
    log: {
      version: "1.2",
      creator: { name: "devsplit", version: "0.1.0" },
      entries: [
        {
          startedDateTime: new Date(d.ts).toISOString(),
          time: wait,
          request: {
            method: d.method,
            url: fullUrl(d),
            httpVersion: "HTTP/1.1",
            cookies: [],
            headers: reqHeaders,
            queryString: harQuery(d.query),
            postData: d.reqBody
              ? { mimeType: reqMime, text: d.reqBody }
              : undefined,
            headersSize: -1,
            bodySize: d.reqSize ?? -1,
          },
          response: {
            status: d.status ?? 0,
            statusText: "",
            httpVersion: "HTTP/1.1",
            cookies: [],
            headers: respHeaders,
            content: {
              size: d.respSize ?? -1,
              mimeType: respMime,
              text: d.respBody ?? "",
            },
            redirectURL: "",
            headersSize: -1,
            bodySize: d.respSize ?? -1,
          },
          cache: {},
          timings: { send: 0, wait, receive: 0 },
        },
      ],
    },
  };
}

/** Save text to a file. In Tauri, opens a native save dialog (webview anchor
 * downloads are unreliable there); in the browser mock, uses a Blob anchor. */
export async function downloadBlob(
  filename: string,
  content: string,
  mime: string,
): Promise<void> {
  if (RUNTIME !== "mock") {
    const [{ save }, { writeTextFile }] = await Promise.all([
      import("@tauri-apps/plugin-dialog"),
      import("@tauri-apps/plugin-fs"),
    ]);
    const path = await save({ defaultPath: filename });
    if (!path) return;
    await writeTextFile(path, content);
    return;
  }
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
