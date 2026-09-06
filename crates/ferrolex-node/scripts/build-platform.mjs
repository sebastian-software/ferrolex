import { spawnSync } from "node:child_process";

const target = process.env.FERROLEX_NODE_TARGET;
const crossCompile = process.env.FERROLEX_NODE_CROSS_COMPILE === "1";
const napi = process.platform === "win32" ? "napi.cmd" : "napi";
const args = ["build", "--platform", "--release"];

if (target) {
  args.push("--target", target);
}
if (crossCompile) {
  args.push("--cross-compile");
}
args.push("--", "--locked");

const result = spawnSync(napi, args, {
  stdio: "inherit",
  shell: process.platform === "win32",
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
