// Documentation capture shares the verified native Python Playwright flow.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const script=fileURLToPath(new URL("../../gui/devtools/evidence-console.py", import.meta.url));
const result=spawnSync(process.env.CURE_PYTHON || "python",[script,"demo"],{stdio:"inherit"});
if(result.error)throw result.error;
process.exit(result.status ?? 1);
