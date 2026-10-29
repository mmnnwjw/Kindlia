# Kindlia Gateway

Cloudflare Worker for the native Kindlia client. See [deployment and configuration](../../docs/GATEWAY.md).

- `npm ci` installs the pinned Wrangler toolchain.
- `npm run check` runs the focused routing/credential/streaming checks.
- `npm run build` bundles without publishing.
- `python ../../tools/deploy_gateway.py --deploy` prepares secrets, publishes and checks the chosen custom domain.

Do not commit `.dev.vars`, `.gateway-secret.json` or generated client config. No runtime storage bindings or paid-only features are required.
