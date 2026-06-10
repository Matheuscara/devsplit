import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // NAO vigiar o target do Rust (senao a pagina recarrega a cada rebuild).
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
