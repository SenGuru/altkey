import { Hono } from "hono";
import { cors } from "hono/cors";
import type { Env } from "./env";
import { proxy } from "./routes/proxy";
import { control } from "./routes/control";
import { sync } from "./routes/sync";
import { webhook } from "./routes/webhook";
import { canary } from "./routes/canary";

const app = new Hono<{ Bindings: Env }>();

app.use(
  "/api/*",
  cors({
    origin: (origin, c) => {
      const allowed = c.env.DASHBOARD_ORIGIN;
      return origin === allowed ? origin : null;
    },
    credentials: true,
    allowMethods: ["GET", "POST", "OPTIONS"],
    allowHeaders: ["content-type", "authorization"],
  }),
);

app.use(
  "/sync/*",
  cors({
    origin: () => "*", // browser extension calls; no credentials required
    allowMethods: ["GET", "POST"],
    allowHeaders: ["content-type"],
  }),
);

app.get("/", (c) =>
  c.json({
    service: "altkey",
    version: "0.1.0",
    docs: c.env.DASHBOARD_ORIGIN || "https://altkey.app/docs",
  }),
);

app.get("/healthz", (c) => c.text(`ok ${Date.now()}`));

app.route("/", proxy);
app.route("/", control);
app.route("/", sync);
app.route("/", webhook);
app.route("/", canary);

app.notFound((c) => c.json({ error: "not_found" }, 404));
app.onError((err, c) => {
  console.error(err);
  return c.json({ error: { message: "internal", type: "internal" } }, 500);
});

export default app;
