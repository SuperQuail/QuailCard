/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

// Tauri 在局域网设备调试时通过该变量指定开发服务器地址。
// @ts-expect-error process 是 Node.js 全局变量。
const host = process.env.TAURI_DEV_HOST;

// 创建适用于 Tauri 开发和构建流程的 Vite 配置。
export default defineConfig(async () => ({
  plugins: [vue(), tailwindcss()],

  // 只扫描应用入口，避免 references/ 等外部仓库的 HTML 被当作入口解析依赖。
  optimizeDeps: {
    entries: ["index.html"],
  },

  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    // Node 24 下 threads 在多测试文件退出阶段可能挂起，单进程 forks 可稳定回收 jsdom。
    pool: "forks",
    fileParallelism: false,
    maxWorkers: 1,
  },

  // 保留 Rust 编译错误输出。
  clearScreen: false,
  // Tauri 需要固定端口，端口被占用时直接失败。
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Rust 文件由 Cargo 监听，Vite 无需重复监听。
      ignored: ["**/src-tauri/**", "**/references/**"],
    },
  },
}));
