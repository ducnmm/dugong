import { createMDX } from "fumadocs-mdx/next";

const withMDX = createMDX();

/** @type {import("next").NextConfig} */
const config = {
  reactStrictMode: true,
  allowedDevOrigins: ["127.0.0.1"],
  images: {
    unoptimized: true
  },
  async rewrites() {
    return [
      {
        source: "/slides/dugong",
        destination: "/slides/dugong.html"
      }
    ];
  }
};

export default withMDX(config);
