import test from "node:test";
import assert from "node:assert/strict";
import { handle, isAllowed, imageAllowed } from "../src/index.js";

const env = { GATEWAY_TOKEN: "test-key-123456789", IMAGE_HOSTS: "*.mitemin.net,*.pximg.net,m.media-amazon.com" };
const root = "https://gateway.example";
const req = (path, init = {}) => new Request(root + path, { ...init, headers: { "x-kindlia-key": env.GATEWAY_TOKEN, ...init.headers } });

test("missing secret and wrong key fail closed without fetching upstream", async () => {
  const noFetch = () => { throw new Error("must not fetch"); };
  assert.equal((await handle(req("/main/api/novel"), {}, noFetch)).status, 503);
  assert.equal((await handle(new Request(root + "/main/api/novel"), env, noFetch)).status, 403);
  assert.equal((await handle(new Request(root + "/health"), {}, noFetch)).status, 200);
});

test("only reader routes and known image hosts are accepted", async () => {
  assert(!isAllowed("main", "/api/wenku/id", "POST"));
  assert(!isAllowed("main", "/api/user/admin", "GET"));
  assert(!isAllowed("main", "/files-wenku/%2e%2e/secret", "GET"));
  assert(!imageAllowed(new URL("https://127.0.0.1/image.png"), env));
  assert(!imageAllowed(new URL("https://mitemin.net.evil.test/image.png"), env));
  assert(!imageAllowed(new URL("https://i.pximg.net:8080/a.png"), env));
  assert(imageAllowed(new URL("https://img1.mitemin.net/a.png"), env));
  const r = await handle(req("/image?url=" + encodeURIComponent("https://evil.test/")), env);
  assert.equal(r.status, 404);
});

test("REST keeps bearer auth and repeated queries; gateway key never reaches origin", async () => {
  const r = await handle(req("/main/api/novel?q=%E4%B9%A6&translations=gpt&translations=youdao", { headers: { authorization: "Bearer reader-session", cookie: "other=private", "x-forwarded-for": "1.2.3.4" } }), env, async (url, init) => {
    assert.equal(new URL(url).origin, "https://n.novelia.cc");
    assert.deepEqual(new URL(url).searchParams.getAll("translations"), ["gpt", "youdao"]);
    assert.equal(init.headers.get("authorization"), "Bearer reader-session");
    assert.equal(init.headers.get("x-kindlia-key"), null);
    assert.equal(init.headers.get("cookie"), null);
    assert.equal(init.headers.get("x-forwarded-for"), null);
    assert.equal(init.redirect, "manual");
    assert.equal(init.cf.cacheTtl, 0);
    return Response.json({ items: [] });
  });
  assert.equal(r.headers.get("cache-control"), "no-store");
  assert.deepEqual(await r.json(), { items: [] });
});

test("EPUB redirection stays on gateway with encoded filename and query intact", async () => {
  const r = await handle(req("/main/api/wenku/id/file/vol.epub"), env, async () => new Response(null, {
    status: 302, headers: { location: "/files-temp/wenku/id.zh.epub?filename=%E4%B9%A6.epub" },
  }));
  assert.equal(r.status, 302);
  assert.equal(r.headers.get("location"), root + "/main/files-temp/wenku/id.zh.epub?filename=%E4%B9%A6.epub");
  const denied = await handle(req("/main/api/wenku/id/file/vol.epub"), env, async () => new Response(null, { status: 302, headers: { location: "https://evil.test/a" } }));
  assert.equal(denied.status, 502);
});

test("partial EPUB streams preserve Range, If-Range and response validators", async () => {
  const r = await handle(req("/main/files-temp/wenku/book.epub", { headers: { range: "bytes=4-", "if-range": '"v1"' } }), env, async (_, init) => {
    assert.equal(init.headers.get("range"), "bytes=4-");
    assert.equal(init.headers.get("if-range"), '"v1"');
    assert.equal(init.headers.get("accept-encoding"), "identity");
    return new Response(new Uint8Array([5, 6, 7, 8]), { status: 206, headers: { "content-range": "bytes 4-7/8", etag: '"v1"', "content-length": "4" } });
  });
  assert.equal(r.status, 206);
  assert.equal(r.headers.get("content-range"), "bytes 4-7/8");
  assert.equal(r.headers.get("etag"), '"v1"');
  assert.equal((await r.arrayBuffer()).byteLength, 4);
});

test("auth forwards refresh cookie only; Set-Cookie and invalid_token challenge survive", async () => {
  const r = await handle(req("/auth/api/v1/auth/refresh?app=n", { method: "POST", headers: { cookie: "other=hidden; refresh-token=fixture", authorization: "Bearer must-not-forward" } }), env, async (url, init) => {
    assert.equal(url, "https://auth.novelia.cc/api/v1/auth/refresh?app=n");
    assert.equal(init.headers.get("cookie"), "refresh-token=fixture");
    assert.equal(init.headers.get("authorization"), null);
    return new Response("session", { headers: { "set-cookie": "refresh-token=new; Domain=auth.novelia.cc; Path=/; HttpOnly; Secure; SameSite=Lax" } });
  });
  assert.match(r.headers.get("set-cookie"), /Path=\/auth/);
  assert.match(r.headers.get("set-cookie"), /HttpOnly/);
  assert(!r.headers.get("set-cookie").includes("Domain="));
  const expired = await handle(req("/main/api/user/favored"), env, async () => new Response(null, { status: 401, headers: { "WWW-Authenticate": 'Bearer error="invalid_token"' } }));
  assert.match(expired.headers.get("www-authenticate"), /invalid_token/);
});

test("book comments use forum service and stream request body", async () => {
  const r = await handle(req("/forum/api/v1/external/comment/novel/web-syosetu-id", { method: "POST", body: '{"content":"test"}', headers: { "content-type": "application/json", authorization: "Bearer comment-session" } }), env, async (url, init) => {
    assert.match(url, /^https:\/\/forum\.novelia\.cc\/api\/v1\/external\/comment/);
    assert.equal(init.headers.get("authorization"), "Bearer comment-session");
    assert.equal(await new Response(init.body).text(), '{"content":"test"}');
    return new Response(null, { status: 204 });
  });
  assert.equal(r.status, 204);
});

test("images cannot receive account credentials and redirects remain proxied", async () => {
  const r = await handle(req("/image?url=" + encodeURIComponent("https://i.pximg.net/a.png"), { headers: { authorization: "Bearer hidden", cookie: "refresh-token=hidden" } }), env, async (_, init) => {
    assert.equal(init.headers.get("authorization"), null);
    assert.equal(init.headers.get("cookie"), null);
    assert.equal(init.headers.get("referer"), "https://www.pixiv.net/");
    return new Response(null, { status: 302, headers: { location: "https://i.pximg.net/b.png" } });
  });
  const redirect = new URL(r.headers.get("location"));
  assert.equal(redirect.origin, root);
  assert.equal(redirect.searchParams.get("url"), "https://i.pximg.net/b.png");
});
