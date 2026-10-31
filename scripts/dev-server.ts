import index from "../index.html";

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 1420,
  development: { hmr: true },
  routes: {
    "/": index,
    "/icons/*": { dir: "./public/icons" },
  },
});

console.log(`Development server: ${server.url}`);
