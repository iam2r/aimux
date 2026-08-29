---
aimux: patch
---

WebDAV `GET` now accepts HTTP `206 Partial Content` alongside `200 OK` when reading the remote manifest/store. Some WebDAV servers (notably the one behind `webdav.iamrazo.eu.org` and a few nginx + gzip/brotli frontends) return `206` for a plain full-resource GET and a `Content-Range` covering the whole body; the previous strict `match 200` only path therefore rejected the second `aimux sync push` (first push writes manifest.json → second push reads it and bails on `HTTP 206`). Mirrors cc-switch's `resp.status().is_success()` handling.
