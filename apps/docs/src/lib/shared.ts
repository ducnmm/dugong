export const appName = "Dugong Docs";

export const siteUrl = process.env.NEXT_PUBLIC_DOCS_URL?.replace(/\/$/, "") ??
  "http://127.0.0.1:3004";

export const dugongAppUrl = process.env.NEXT_PUBLIC_DUGONG_APP_URL?.replace(/\/$/, "") ??
  "http://localhost:43173";

export const docsRoute = "/";
