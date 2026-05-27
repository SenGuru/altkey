export interface Env {
  DB: D1Database;

  CF_SECRET: string;
  POLAR_API_KEY: string;
  POLAR_WEBHOOK_SECRET: string;
  NOWPAYMENTS_API_KEY: string;
  NOWPAYMENTS_IPN_SECRET: string;
  CANARY_PRIVATE_KEY: string;

  DASHBOARD_ORIGIN: string;
  CANARY_PUBLIC_KEY: string;
}
