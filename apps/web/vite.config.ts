import tailwindcss from "@tailwindcss/vite";
import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const here = fileURLToPath(new URL(".", import.meta.url));
const kunkunSdkLoader = process.env.SPACE_LENS_KUNKUN_BUILD === "1"
  ? resolve(here, "src/lib/api/kunkun-sdk-loader.kunkun.ts")
  : resolve(here, "src/lib/api/kunkun-sdk-loader.ts");

export default defineConfig({
  resolve: {
    alias: {
      "#space-lens/kunkun-sdk-loader": kunkunSdkLoader,
    },
  },
  plugins: [
    tailwindcss(),
    sveltekit({
      compilerOptions: {
        // Force runes mode for the project, except for libraries. Can be removed in svelte 6.
        runes: ({ filename }) =>
          filename.split(/[/\\]/).includes("node_modules") ? undefined : true,
      },
      adapter: adapter(),
      router: {
        type: "hash",
        resolution: "client",
      },
    }),
  ],
});
