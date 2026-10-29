// Kindlia 原生客户端专用入口；不解析小说 JSON、不缓冲 EPUB、不提供任意 URL 转发。
const ORIGINS = Object.freeze({
  main: "https://n.novelia.cc",
  auth: "https://auth.novelia.cc",
  forum: "https://forum.novelia.cc",
});
const READ = new Set(["GET", "HEAD"]);
const HOP_HEADERS = ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade"];
const REQUEST_HEADERS = ["accept", "content-type", "authorization", "range", "if-range", "if-none-match", "if-modified-since"];

function error(status, message) {
  return Response.json({ error: message }, { status, headers: { "cache-control": "no-store" } });
}

function validPath(path) {
  try {
    const decoded = decodeURIComponent(path);
    return !decoded.includes("\\") && !/[\x00-\x1f\x7f]/.test(decoded)
      && !decoded.split("/").some(p => p === "." || p === "..");
  } catch { return false; }
}

export function isAllowed(kind, path, method) {
  if (!validPath(path)) return false;
  if (kind === "auth") return method === "POST" && /^\/api\/v1\/auth\/(login|refresh|logout)$/.test(path);
  if (kind === "forum") return ["GET", "HEAD", "POST", "PATCH", "DELETE"].includes(method)
    && /^\/api\/v1\/external\/comment\/novel(?:\/|$)/.test(path);
  if (kind !== "main") return false;
  if (/^\/api\/(novel|wenku)(?:\/|$)/.test(path)) return READ.has(method);
  if (/^\/api\/user\/(favored|favored-web|favored-wenku|read-history)(?:\/|$)/.test(path)) {
    return ["GET", "HEAD", "POST", "PUT", "DELETE"].includes(method);
  }
  return READ.has(method) && /^\/(files-wenku|files-temp\/wenku|files-extra)\//.test(path);
}

export function imageAllowed(url, env) {
  if (url.protocol !== "https:" || url.username || url.password || url.port || url.hash) return false;
  const host = url.hostname.toLowerCase();
  // 精确域名或显式 *.suffix；不允许通配所有主机、IP、localhost、自定义端口。
  if (host === "localhost" || host.includes(":") || /^[\d.]+$/.test(host)) return false;
  return (env.IMAGE_HOSTS || "").split(",").map(s => s.trim().toLowerCase()).filter(Boolean).some(rule =>
    rule.startsWith("*.") ? host.endsWith(rule.slice(1)) && host.length > rule.length - 1 : host === rule
  );
}

function route(url, env) {
  if (url.pathname === "/image") {
    if (url.searchParams.getAll("url").length !== 1) return null;
    let target;
    try { target = new URL(url.searchParams.get("url")); } catch { return null; }
    return imageAllowed(target, env) ? { kind: "image", target } : null;
  }
  const match = /^\/(main|auth|forum)(\/.*)$/.exec(url.pathname);
  if (!match) return null;
  const [, kind, path] = match;
  // 拼接固定 origin，避免 //host、URL 解码或客户端输入改变上游主机。
  const target = new URL(ORIGINS[kind]);
  target.pathname = path;
  target.search = url.search;
  return { kind, target };
}

export function rewriteLocation(location, upstream, gateway, kind, env) {
  let target;
  try { target = new URL(location, upstream); } catch { return null; }
  if (kind === "image") {
    if (!imageAllowed(target, env)) return null;
    const out = new URL("/image", gateway);
    out.searchParams.set("url", target.href);
    return out.href;
  }
  const found = Object.entries(ORIGINS).find(([, origin]) => origin === target.origin);
  if (!found || !isAllowed(found[0], target.pathname, "GET")) return null;
  return new URL(`/${found[0]}${target.pathname}${target.search}`, gateway).href;
}

function cookies(headers) {
  // Workers 使用 getAll，Node 的本地测试使用 getSetCookie；多个 Set-Cookie 不做逗号拆分。
  if (headers.getSetCookie) return headers.getSetCookie();
  if (headers.getAll) return headers.getAll("set-cookie");
  const value = headers.get("set-cookie");
  return value ? [value] : [];
}

export async function handle(request, env, fetchOrigin = fetch) {
  const url = new URL(request.url);
  if (url.pathname === "/health" && request.method === "GET") {
    return Response.json({ service: "kindlia-gateway", version: 1, authenticated: Boolean(env.GATEWAY_TOKEN) }, { headers: { "cache-control": "no-store" } });
  }
  if (!env.GATEWAY_TOKEN) return error(503, "Set the GATEWAY_TOKEN Worker secret first");
  if (request.headers.get("x-kindlia-key") !== env.GATEWAY_TOKEN) return error(403, "Invalid gateway key");
  const dest = route(url, env);
  if (!dest || (dest.kind === "image" ? !READ.has(request.method) : !isAllowed(dest.kind, dest.target.pathname, request.method))) {
    return error(404, "Route or method not allowed");
  }
  const headers = new Headers();
  for (const key of REQUEST_HEADERS) {
    if (dest.kind === "image" && key === "authorization") continue;
    const value = request.headers.get(key);
    if (value !== null) headers.set(key, value);
  }
  // Kindle 不需要浏览器 CORS；不转发代理密钥、客户端 IP、任意 Cookie 或凭证到图片站。
  if (dest.kind === "auth") {
    headers.delete("authorization");
    const refresh = (request.headers.get("cookie") || "").split(";").map(s => s.trim()).find(s => s.startsWith("refresh-token="));
    if (refresh) headers.set("cookie", refresh);
  }
  headers.set("user-agent", "Kindlia-Gateway/1");
  headers.set("accept-encoding", "identity");
  if (dest.kind === "image" && dest.target.hostname.endsWith("pximg.net")) headers.set("referer", "https://www.pixiv.net/");
  let response;
  try {
    response = await fetchOrigin(dest.target.href, {
      method: request.method, headers,
      body: READ.has(request.method) ? undefined : request.body,
      redirect: "manual", // 由重写后的 Location 让客户端继续；避免请求绕过反代或凭证外泄。
      cf: { cacheTtl: 0, cacheEverything: false },
    });
  } catch { return error(502, "Upstream connection failed"); }
  const outgoing = new Headers(response.headers);
  for (const h of HOP_HEADERS) outgoing.delete(h);
  outgoing.delete("set-cookie");
  if (dest.kind === "auth") {
    for (const cookie of cookies(response.headers)) {
      // 保留 HttpOnly/Secure/SameSite/Expires 等属性；移除源站 Domain 并缩小 Path。
      const parts = cookie.split(";").filter(p => !/^\s*(domain|path)\s*=/i.test(p));
      outgoing.append("set-cookie", `${parts.join(";")}; Path=/auth`);
    }
  }
  outgoing.set("cache-control", "no-store");
  outgoing.set("x-kindlia-gateway", "1");
  const location = response.headers.get("location");
  if (location) {
    const rewritten = rewriteLocation(location, dest.target, url, dest.kind, env);
    if (!rewritten) { await response.body?.cancel(); return error(502, "Upstream redirect outside allowed routes"); }
    outgoing.set("location", rewritten);
  }
  // 原样流转，保留 ETag / Content-Range / Content-Length / WWW-Authenticate。
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers: outgoing });
}

export default { fetch: (request, env) => handle(request, env) };
