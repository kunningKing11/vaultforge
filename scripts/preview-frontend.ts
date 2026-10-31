const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 4173,
  routes: { "/*": { dir: "./dist" } },
});

console.log(`Preview server: ${server.url}`);
