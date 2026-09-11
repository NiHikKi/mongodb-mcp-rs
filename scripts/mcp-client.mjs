import { spawn } from "node:child_process";
import readline from "node:readline";

const bin = process.argv[2];
const steps = JSON.parse(process.argv[3]);
const child = spawn(bin, [], { env: { ...process.env }, stdio: ["pipe", "pipe", "ignore"] });
const rl = readline.createInterface({ input: child.stdout });

const pending = new Map();
rl.on("line", (line) => {
  if (!line.trim().startsWith("{")) return;
  let m; try { m = JSON.parse(line); } catch { return; }
  if (m.id !== undefined && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
});

let nextId = 1;
function send(method, params) {
  const id = nextId++;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  });
}
function notify(method) {
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method }) + "\n");
}

await send("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "seq", version: "1" } });
notify("notifications/initialized");

const list = await send("tools/list", {});
console.log(`tools/list: ${list.result.tools.length} tools`);

let ok = 0, bad = 0;
for (const step of steps) {
  const res = await send("tools/call", { name: step.name, arguments: step.args });
  const text = res.result?.content?.[0]?.text ?? JSON.stringify(res.error ?? {});
  const failed = Boolean(res.result?.isError || res.error);
  const expected = step.expectError === true;
  const good = failed === expected;
  if (good) ok++; else bad++;
  const label = good ? (expected ? "ok(refused)" : "ok         ") : "UNEXPECTED ";
  console.log(`${label} ${(step.label ?? step.name).padEnd(26)} ${text.slice(0, 110).replace(/\s+/g, " ")}`);
}
console.log(`\npassed: ${ok}  unexpected: ${bad}`);
child.stdin.end();
child.kill();
process.exit(bad ? 1 : 0);
