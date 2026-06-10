// Pure-JS JWT helpers — base64url codec plus decode/encode.
//
// Decode is used by the Session view to inspect captured Bearer tokens; encode
// is used by the headless MOCK ipc to mint realistic, decodable tokens.

function base64UrlDecode(segment: string): string {
  const padLength = segment.length % 4 === 0 ? 0 : 4 - (segment.length % 4);
  const b64 = segment.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat(padLength);
  const binary = atob(b64);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function base64UrlEncode(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export interface JwtDecoded {
  raw: string;
  header: Record<string, unknown>;
  payload: Record<string, unknown>;
}

export function decodeJwt(token: string): JwtDecoded | null {
  const parts = token.split(".");
  if (parts.length < 2) return null;
  try {
    const header = JSON.parse(base64UrlDecode(parts[0])) as Record<string, unknown>;
    const payload = JSON.parse(base64UrlDecode(parts[1])) as Record<string, unknown>;
    if (typeof header !== "object" || header === null) return null;
    if (typeof payload !== "object" || payload === null) return null;
    return { raw: token, header, payload };
  } catch {
    return null;
  }
}

export function encodeJwt(
  header: Record<string, unknown>,
  payload: Record<string, unknown>,
): string {
  const h = base64UrlEncode(JSON.stringify(header));
  const p = base64UrlEncode(JSON.stringify(payload));
  // Signature is decorative in the mock — never verified client-side.
  const sig = base64UrlEncode(`mock-sig-${header.alg ?? "none"}`);
  return `${h}.${p}.${sig}`;
}
