import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  allowedDevOrigins: ["172.16.0.13"],
  experimental: {
    optimizePackageImports: ["lucide-react", "react-icons"],
    serverActions: {
      allowedOrigins: ["172.16.0.13"],
    },
  },
};

export default nextConfig;
