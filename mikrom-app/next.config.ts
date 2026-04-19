import type { NextConfig } from "next";
import withFlowbiteReact from "flowbite-react/plugin/nextjs";

const nextConfig: NextConfig = {
  output: "standalone",
  allowedDevOrigins: ["172.16.0.13"],
};

export default withFlowbiteReact(nextConfig);