// Compatibility entry point; maintained checks use native Python Playwright.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const script = fileURLToPath(new URL("./evidence-console.py", import.meta.url));
const result = spawnSync(process.env.CURE_PYTHON || "python", [script, "verify"], { stdio: "inherit" });
if(result.error) throw result.error;
process.exit(result.status ?? 1);
