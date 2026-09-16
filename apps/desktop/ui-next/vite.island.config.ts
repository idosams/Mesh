import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  plugins: [tailwindcss()],
  build: {
    outDir: new URL("../ui/review-workbench-next", import.meta.url).pathname,
    emptyOutDir: true,
    sourcemap: false,
    lib: {
      entry: new URL("./src/island.tsx", import.meta.url).pathname,
      formats: ["es"],
      fileName: () => "review-workbench-island.js",
    },
  },
});
