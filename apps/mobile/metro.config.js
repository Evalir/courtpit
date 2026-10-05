// Metro for the Expo app. Expo configures the npm-workspace monorepo on its own (SDK 52+); the
// only addition is a development proxy: requests to `/api/*` and `/healthz` on the dev server
// are forwarded to the Racquet Collective API (`RACQUETCOLLECTIVE_DEV_API`, default http://127.0.0.1:8080). The web
// app and the API therefore share an origin in development as they do in production, so the
// session cookie works, and a phone running Expo Go reaches the API through the same host it
// loaded the bundle from.
const http = require("node:http");
const https = require("node:https");
const { getDefaultConfig } = require("expo/metro-config");

const config = getDefaultConfig(__dirname);
const upstream = new URL(process.env.RACQUETCOLLECTIVE_DEV_API ?? "http://127.0.0.1:8080");
const transport = upstream.protocol === "https:" ? https : http;

function isApi(url) {
  return url.startsWith("/api/") || url === "/healthz" || url === "/readyz";
}

function proxy(req, res) {
  const forward = transport.request(
    {
      protocol: upstream.protocol,
      hostname: upstream.hostname,
      port: upstream.port,
      method: req.method,
      path: req.url,
      // The app sends X-RacquetCollective-Community in development, so the API needs no community Host.
      headers: { ...req.headers, host: upstream.host },
    },
    (answer) => {
      res.writeHead(answer.statusCode ?? 502, answer.headers);
      answer.pipe(res);
    },
  );
  forward.on("error", (error) => {
    res.writeHead(502, { "content-type": "application/json" });
    res.end(
      JSON.stringify({
        error: {
          code: "dev_proxy",
          message: `API unreachable at ${upstream.origin}: ${error.message}`,
        },
      }),
    );
  });
  req.pipe(forward);
}

config.server = {
  ...config.server,
  enhanceMiddleware: (middleware) => (req, res, next) =>
    req.url && isApi(req.url) ? proxy(req, res) : middleware(req, res, next),
};

module.exports = config;
