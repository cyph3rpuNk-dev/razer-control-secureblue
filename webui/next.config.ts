import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Tauri serves the exported static bundle from ../out.
  output: "export",
  images: { unoptimized: true },
};

export default nextConfig;
