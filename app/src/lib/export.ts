// curl + HAR serializers for a single captured request, plus a Blob download.

import type { RequestDetail } from "./ipc.ts";

function headerValue(headers: Array<[string, string]>, name: string): string | undefined {
  const lower = name.toLowerCase();
  for (const [k, v] of headers) {
    if (k.toLowerCase() === lower) return v;
  }
  return undefined;
}

export function toCurl(d: RequestDetail): string {
  const url = `https://${d.host}${d.path}`;
  const lines = [`curl -X ${d.method} '${url}'`];
  for (const [k, v] of d.reqHeaders) {
    lines.push(`  -H '${k}: ${v.replace(/'/g, "'\\''")}'`);
  }
  if (d.reqBody) {
    lines.push(`  --data-raw '${d.reqBody.replace(/'/g, "'\\''")}'`);
  }
  return lines.join(" \\\n");
}

interface HarNameValue {
  name: string;
  value: string;
}

export function toHar(d: RequestDetail): unknown {
  const url = `https://${d.host}${d.path}`;
  const reqHeaders: HarNameValue[] = d.reqHeaders.map(([name, value]) => ({ name, value }));
  const respHeaders: HarNameValue[] = d.respHeaders.map(([name, value]) => ({ name, value }));
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
            url,
            httpVersion: "HTTP/1.1",
            cookies: [],
            headers: reqHeaders,
            queryString: [],
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

export function downloadBlob(filename: string, content: string, mime: string): void {
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
