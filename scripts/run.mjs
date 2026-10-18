/**
 * Loads build-env.json (optional) and runs a command with the right environment.
 *
 * On machines with a normal system drive this file does not exist and the
 * command runs with the unmodified process environment. On machines where the
 * system drive is full, build-env.json points the Rust toolchain, crate
 * registry and build output at a build root outside the app source. Keeping
 * the paths in one JSON file means the PowerShell helper and the npm scripts
 * can never disagree.
 *
 * Usage: node scripts/run.mjs <command> [args...]
 */
import { existsSync, readFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const env = { ...process.env };

try {
  const envPath = join(root, "build-env.json");
  if (existsSync(envPath)) {
    const cfg = JSON.parse(readFileSync(envPath, "utf8"));
    env.RUSTUP_HOME = join(cfg.buildRoot, "rustup");
    env.CARGO_HOME = join(cfg.buildRoot, "cargo");
    env.CARGO_TARGET_DIR = join(cfg.buildRoot, "target");
    env.TEMP = join(cfg.buildRoot, "tmp");
    env.TMP = join(cfg.buildRoot, "tmp");
    if (cfg.buildJobs) env.CARGO_BUILD_JOBS = String(cfg.buildJobs);
    // The GNU toolchain on Windows needs an explicit linker; cargo picks this
    // up via -C linker=... (verified with `cargo build -vv` on a probe crate).
    // No CARGO_BUILD_TARGET: forcing a target triple would move output to
    // target/<triple>/release and invalidate the warm target/release cache.
    if (cfg.linker) env.CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = cfg.linker;

    // Windows environment variable names are case-insensitive to the OS but
    // case-sensitive inside Node, and the system here spells it `Path`.
    // Writing a second `PATH` entry would shadow the real one and break every
    // child process, so the existing key is updated in place.
    const pathKey = Object.keys(env).find((k) => k.toUpperCase() === "PATH") ?? "PATH";
    env[pathKey] = [join(cfg.buildRoot, "cargo", "bin"), ...(cfg.extraPath ?? []), env[pathKey]]
      .filter(Boolean)
      .join(delimiter);
  }
} catch {
  // Missing or unreadable build-env.json: fall through with process.env.
}

const [command, ...args] = process.argv.slice(2);
if (!command) {
  console.error("usage: node scripts/run.mjs <command> [args...]");
  process.exit(2);
}

// `.cmd` shims (npm, npx) need a shell on Windows; native executables do not.
const isShellCommand = /\.(cmd|bat)$/i.test(command) || !command.includes("\\");

const child = isShellCommand
  ? spawn(command, args, { stdio: "inherit", env, shell: true })
  : spawn(command, args, { stdio: "inherit", env });

child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 1);
});