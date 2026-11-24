// The desktop E2E platform layer (E2E-PLATFORM): the only module that names an operating
// system or branches on one, and only for what really differs between them:
// - the primary shortcut modifier and its label (⌘ on macOS, Ctrl elsewhere);
// - executable file names, '/' text paths and paths that cannot be read as URLs;
// - environment variable names, case-insensitive on Windows, and where each OS keeps a
//   user's home, data and web storage;
// - how a command is found on PATH (a .cmd shim on Windows, an executable script elsewhere),
//   how pnpm is started, and how a process tree is stopped;
// - Linux runs the runner inside its own dbus-run-session and xvfb-run, on X11;
// - a Windows desktop gives page focus events only to its one foreground window, so Windows
//   runs one app at a time and gives its window the focus; macOS keeps covered windows drawing;
// - where the app finds the system browser's opener (a command on PATH on Unix, the test
//   build's opener seam on Windows). The stand-in it runs is the same on every OS.
import type {} from "@wdio/tauri-service";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, readFileSync, readdirSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { PNG } from "pngjs";
import { homedir, release } from "node:os";
import { setTimeout as delay } from "node:timers/promises";
import { basename, delimiter, isAbsolute, join, relative, resolve, sep, toNamespacedPath } from "node:path";
import { callWindow, makeDirectory, nativePath, newDirectory, readTextIfExists, removeTree, replaceText, scratchDirectory, writeText, type NativePath } from "./portable.ts";

const WINDOWS = process.platform === "win32";
const MACOS = process.platform === "darwin";
const LINUX = process.platform === "linux";

/** The modifier of the app's primary shortcuts as a person on this OS presses it. */
export const primaryModifier: "Meta" | "Control" = MACOS ? "Meta" : "Control";

/** A shortcut as the app labels it on this OS. */
export function shortcutLabel(keys: string): string {
  return MACOS ? keys.replaceAll("Ctrl+", "⌘") : keys;
}

/** This Node.js executable: fixtures declare it as the program the product runs. */
export const nodeProgram = process.execPath as NativePath;

/** The file name of an executable built for this OS. */
export function executableName(name: string): string {
  return WINDOWS ? `${name}.exe` : name;
}

/** The `/`-separated text form of a native path, as the product shows and stores paths. */
export function portablePath(path: NativePath): string {
  return WINDOWS ? path.replaceAll(sep, "/") : path;
}

/** The repository-relative form of `path` as the app shows it. */
export function relativeTextPath(root: NativePath, path: NativePath): string {
  const within = relative(root, path);
  if (within === "" || within.startsWith("..") || isAbsolute(within)) throw new Error(`path ${path} is not inside ${root}`);
  return portablePath(within as NativePath);
}

/** A folder for a setting that takes a URL or a path. A Windows drive path such as C:\x also
 * parses as a URL with the scheme "c", so it is given in its namespaced form, which only a
 * path can be; other systems' absolute paths are no URL. */
export function pathNotUrl(path: NativePath): string {
  return toNamespacedPath(path);
}

/** Whether two native paths refer to the same file on this OS. */
export function samePath(left: NativePath, right: NativePath): boolean {
  const canonical = (path: NativePath): string => {
    try { return realpathSync.native(path); } catch { return resolve(path); }
  };
  const a = canonical(left);
  const b = canonical(right);
  return WINDOWS ? a.toLowerCase() === b.toLowerCase() : a === b;
}

// ---------------------------------------------------------------------------------
// Environment

export type Environment = NodeJS.ProcessEnv;

function sameName(left: string, right: string): boolean {
  return WINDOWS ? left.toUpperCase() === right.toUpperCase() : left === right;
}

/** A variable of this process; Windows variable names ignore case. */
export function variable(name: string): string | undefined {
  const found = Object.keys(process.env).find((key) => sameName(key, name));
  return found === undefined ? undefined : process.env[found];
}

/** Set (or with `undefined`, remove) a variable of this process. */
export function setVariable(name: string, value: string | undefined): void {
  for (const key of Object.keys(process.env)) if (sameName(key, name)) delete process.env[key];
  if (value !== undefined) process.env[name] = value;
}

function hasVariable(env: Environment, name: string): boolean {
  return Object.keys(env).some((key) => sameName(key, name) && env[key] !== undefined);
}

/** `env` with `name` set to `value`, removing differently spelled Windows aliases. */
export function withVariable(env: Environment, name: string, value: string): Environment {
  const copy = { ...env };
  for (const key of Object.keys(copy)) if (sameName(key, name)) delete copy[key];
  copy[name] = value;
  return copy;
}

/** A copy of this process's environment for one fixture: variables `drop` selects (by
 * upper-case name) are removed, and every place an OS keeps a user's home, configuration,
 * data, cache, web storage and temporary files, and the user's name, point inside `root`. */
export function isolatedEnvironment(root: NativePath, drop: (name: string) => boolean): Environment {
  let env: Environment = {};
  for (const [name, value] of Object.entries(process.env)) {
    if (!drop(name.toUpperCase()) && !["HOMEDRIVE", "HOMEPATH", "USER", "USERNAME", "LOGNAME"].includes(name.toUpperCase())) env[name] = value;
  }
  const folders: Record<string, string> = {
    HOME: "home", USERPROFILE: "home", APPDATA: "appdata", LOCALAPPDATA: "localappdata",
    XDG_CONFIG_HOME: "config", XDG_DATA_HOME: "data", XDG_STATE_HOME: "user-state", XDG_CACHE_HOME: "cache",
    XDG_RUNTIME_DIR: "runtime", TMPDIR: "tmp", TEMP: "tmp", TMP: "tmp", GNUPGHOME: "gnupg",
    // WebView2 finds its storage through the Windows known folders, which ignore the
    // variables above; this one takes precedence over them.
    ...(WINDOWS ? { WEBVIEW2_USER_DATA_FOLDER: "webview" } : {}),
  };
  for (const [name, folder] of Object.entries(folders)) env = withVariable(env, name, makeDirectory(nativePath(root, folder), { private: true }));
  // WKWebView keeps its website data under the CoreFoundation home, which ignores HOME.
  if (MACOS) env.CFFIXED_USER_HOME = env.HOME;
  const home = nativePath(root, "home");
  const drive = WINDOWS ? home.slice(0, home.indexOf(sep)) : "";
  return { ...env, ...(WINDOWS ? { HOMEDRIVE: drive, HOMEPATH: home.slice(drive.length) } : {}), USER: "alice", USERNAME: "alice", LOGNAME: "alice" };
}

function programPath(env: Environment): string[] {
  const key = Object.keys(env).find((name) => sameName(name, "PATH"));
  return (key === undefined ? "" : env[key] ?? "").split(delimiter).filter(Boolean);
}

/** `env` with PATH starting with `folders`, then those of its PATH that `keep` accepts. */
export function withPathFirst(env: Environment, folders: string[], keep: (folder: string) => boolean = () => true): Environment {
  return withVariable(env, "PATH", [...folders, ...programPath(env).filter(keep)].join(delimiter));
}

// ---------------------------------------------------------------------------------
// Commands and processes

/** Write the command `name` into `bin` as this OS finds commands on PATH (a `.cmd` shim on
 * Windows, an executable script elsewhere): it runs DESKTOP_TEST_NODE with the script the
 * variable `scriptVariable` names, passing its arguments on. Returns the command's file name. */
export function writeNodeCommand(bin: NativePath, name: string, scriptVariable: string): string {
  if (WINDOWS) {
    writeText(nativePath(bin, `${name}.cmd`), `@"%DESKTOP_TEST_NODE%" "%${scriptVariable}%" %*\r\n`);
    return `${name}.cmd`;
  }
  writeFileSync(join(bin, name), `#!/bin/sh\nexec "$DESKTOP_TEST_NODE" "$${scriptVariable}" "$@"\n`, { mode: 0o755 });
  return name;
}

/** Whether `folder` holds a program a shell would run as `name`. */
export function folderHoldsCommand(folder: string, name: string): boolean {
  try {
    if (WINDOWS) {
      const extensions = (variable("PATHEXT") ?? ".COM;.EXE;.BAT;.CMD").split(";").filter(Boolean).map((ext) => ext.toLowerCase());
      return readdirSync(folder).some((entry) => extensions.some((ext) => entry.toLowerCase() === `${name}${ext}`));
    }
    const path = join(folder, name);
    return existsSync(path) && (statSync(path).mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

/** pnpm as this run can start it: the entry point pnpm passed to this script, else (a Human
 * tool started by node, or a script another package manager started, whose entry point is not
 * pnpm's) pnpm on PATH, through its JS entry on Windows, whose .cmd shim Node cannot spawn
 * without a shell. */
export function packageCommand(args: string[]): { program: string; args: string[] } {
  const entry = variable("npm_execpath");
  if (entry && isAbsolute(entry)) {
    if (/^pnpm\.(?:c|m)?js$/i.test(basename(entry))) return { program: process.execPath, args: [entry, ...args] };
    if (/^pnpm(?:\.exe)?$/i.test(basename(entry))) return { program: entry, args };
  }
  if (WINDOWS) {
    for (const folder of programPath(process.env)) {
      if (existsSync(join(folder, "pnpm.exe"))) return { program: join(folder, "pnpm.exe"), args };
      // pnpm installed by npm, or Corepack's pnpm beside node.
      for (const script of [join(folder, "node_modules", "pnpm", "bin", "pnpm.cjs"), join(folder, "node_modules", "corepack", "dist", "pnpm.js")]) {
        if (existsSync(script)) return { program: process.execPath, args: [script, ...args] };
      }
    }
    throw new Error("pnpm is missing: install pnpm, or start through pnpm");
  }
  return { program: "pnpm", args };
}

// Keep a native Job Object handle in the wrapper, not in the command that might finish
// before its children. The wrapper joins before starting any command, so no child can race
// assignment. Killing the wrapper closes the last non-inheritable handle and kills the whole
// job, even when the command's own PID has already gone away. No shell interprets the
// command's arguments. Compiled once per run into a standalone .exe so batches avoid
// repeated PowerShell and C# compiler startup, and so the wrapper remains the batch Node
// process's direct parent (batchJobPids / checkForeground).
//
// After a build root exits, Cargo intentionally allows long-lived toolchain daemons (such as
// mspdbsrv.exe) to outlive the build; waiting blindly on ActiveProcesses > 1 stalls for their
// 15-minute idle timeout. Instead, inspect the job's PID list via JobObjectBasicProcessIdList
// (class 3) and stop waiting once every remaining descendant is on the build-daemon allowlist,
// relying on the per-build private _MSPDBSRV_ENDPOINT_ so kill-on-close never disrupts
// concurrent builds. Any other descendant (such as a stdout holder in runner.test.ts) keeps
// the wrapper waiting, and a wait past 10s logs the remaining image names to stderr.
const WINDOWS_JOB_SOURCE = String.raw`
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class E2eBuildJob {
  private const uint KILL_ON_JOB_CLOSE = 0x2000;
  private const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
  private const int JOB_OBJECT_BASIC_PROCESS_ID_LIST = 3;
  private const int JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9;
  private const int ERROR_MORE_DATA = 234;
  private const int ERROR_INVALID_PARAMETER = 87;
  private const int WAIT_DIAGNOSTIC_MS = 10000;

  [StructLayout(LayoutKind.Sequential)] public struct Limits {
    public long ProcessTime, JobTime;
    public uint Flags;
    public UIntPtr MinSet, MaxSet;
    public uint ProcessLimit;
    public UIntPtr Affinity;
    public uint Priority, Scheduling;
  }

  [StructLayout(LayoutKind.Sequential)] public struct Extended {
    public Limits Basic;
    public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes;
    public UIntPtr ProcessLimit, JobLimit, PeakProcess, PeakJob;
  }

  [DllImport("kernel32.dll", SetLastError = true)]
  private static extern IntPtr CreateJobObject(IntPtr attributes, IntPtr name);

  [DllImport("kernel32.dll", SetLastError = true)]
  private static extern bool SetInformationJobObject(IntPtr job, int type, ref Extended info, uint size);

  [DllImport("kernel32.dll", SetLastError = true)]
  private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

  [DllImport("kernel32.dll", SetLastError = true)]
  private static extern bool QueryInformationJobObject(IntPtr job, int type, IntPtr info, uint size, out uint returned);

  [DllImport("kernel32.dll", SetLastError = true)]
  private static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);

  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  private static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder buffer, ref uint size);

  [DllImport("kernel32.dll", SetLastError = true)]
  private static extern bool CloseHandle(IntPtr handle);

  private static uint[] ListJobPids(IntPtr job) {
    int capacity = 64;
    while (true) {
      int size = 8 + capacity * IntPtr.Size;
      IntPtr buffer = Marshal.AllocHGlobal(size);
      try {
        Marshal.WriteInt32(buffer, 0, 0);
        Marshal.WriteInt32(buffer, 4, 0);
        uint returned;
        if (QueryInformationJobObject(job, JOB_OBJECT_BASIC_PROCESS_ID_LIST, buffer, (uint)size, out returned)) {
          int assigned = Marshal.ReadInt32(buffer, 0);
          int count = Marshal.ReadInt32(buffer, 4);
          if (assigned > count) {
            capacity = Math.Max(assigned + 16, capacity * 2);
            continue;
          }
          uint[] pids = new uint[count];
          for (int i = 0; i < count; i++) {
            pids[i] = unchecked((uint)Marshal.ReadIntPtr(buffer, 8 + i * IntPtr.Size).ToInt64());
          }
          return pids;
        }
        int err = Marshal.GetLastWin32Error();
        if (err == ERROR_MORE_DATA) {
          int assigned = Marshal.ReadInt32(buffer, 0);
          capacity = Math.Max(assigned + 16, capacity * 2);
          continue;
        }
        throw new InvalidOperationException("cannot observe E2E build Job Object (Win32 error " + err + ")");
      } finally {
        Marshal.FreeHGlobal(buffer);
      }
    }
  }

  private static string ProcessImageName(uint pid) {
    IntPtr handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    if (handle == IntPtr.Zero) {
      int err = Marshal.GetLastWin32Error();
      if (err == ERROR_INVALID_PARAMETER) return null;
      return "<pid:" + pid + ">";
    }
    try {
      StringBuilder sb = new StringBuilder(1024);
      uint len = (uint)sb.Capacity;
      if (!QueryFullProcessImageNameW(handle, 0, sb, ref len)) {
        return "<pid:" + pid + ">";
      }
      string full = sb.ToString(0, (int)len);
      string name = Path.GetFileName(full);
      return string.IsNullOrEmpty(name) ? full : name;
    } finally {
      CloseHandle(handle);
    }
  }

  private static bool IsAllowedDaemon(string imageName) {
    return string.Equals(imageName, "mspdbsrv.exe", StringComparison.OrdinalIgnoreCase);
  }

  public static int Main() {
    try {
      IntPtr job = CreateJobObject(IntPtr.Zero, IntPtr.Zero);
      if (job == IntPtr.Zero) {
        Console.Error.WriteLine("cannot create E2E build Job Object");
        return 1;
      }
      Extended limits = new Extended();
      limits.Basic.Flags = KILL_ON_JOB_CLOSE;
      uint limitSize = (uint)Marshal.SizeOf(limits);
      if (!SetInformationJobObject(job, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION, ref limits, limitSize)) {
        Console.Error.WriteLine("cannot set kill-on-close for the E2E build Job Object");
        return 1;
      }
      Process self = Process.GetCurrentProcess();
      uint selfPid = unchecked((uint)self.Id);
      if (!AssignProcessToJobObject(job, self.Handle)) {
        Console.Error.WriteLine("cannot enter the E2E build Job Object");
        return 1;
      }

      string program = Environment.GetEnvironmentVariable("DESKTOP_BUILD_PROGRAM");
      string args = Environment.GetEnvironmentVariable("DESKTOP_BUILD_ARGS") ?? "";
      bool completesWithRoot = Environment.GetEnvironmentVariable("DESKTOP_TREE_COMPLETES_WITH_ROOT") == "1";
      if (string.IsNullOrEmpty(program)) {
        Console.Error.WriteLine("DESKTOP_BUILD_PROGRAM is not set");
        return 1;
      }

      ProcessStartInfo info = new ProcessStartInfo();
      info.FileName = program;
      info.Arguments = args;
      info.UseShellExecute = false;
      info.EnvironmentVariables.Remove("DESKTOP_BUILD_PROGRAM");
      info.EnvironmentVariables.Remove("DESKTOP_BUILD_ARGS");
      info.EnvironmentVariables.Remove("DESKTOP_TREE_COMPLETES_WITH_ROOT");
      if (completesWithRoot) {
        info.EnvironmentVariables["DESKTOP_TEST_BATCH_JOB_PID"] = self.Id.ToString();
      }

      int code;
      using (Process child = Process.Start(info)) {
        if (child == null) {
          Console.Error.WriteLine("cannot start E2E build child process");
          return 1;
        }
        child.WaitForExit();
        code = child.ExitCode;
      }

      // A batch owns service teardown; if it exits leaving apps behind, close the job as a
      // fail-safe instead of waiting forever. Build commands drain their descendants except
      // isolated toolchain daemons (mspdbsrv.exe) on the allowlist.
      if (!completesWithRoot) {
        Stopwatch wait = Stopwatch.StartNew();
        bool loggedSlowWait = false;
        while (true) {
          uint[] pids = ListJobPids(job);
          List<string> remaining = new List<string>();
          bool hasUnlisted = false;
          bool sawTransientExit = false;
          for (int i = 0; i < pids.Length; i++) {
            uint pid = pids[i];
            if (pid == selfPid) continue;
            string name = ProcessImageName(pid);
            if (name == null) {
              sawTransientExit = true;
              continue;
            }
            remaining.Add(name + " (pid " + pid + ")");
            if (!IsAllowedDaemon(name)) hasUnlisted = true;
          }
          if (remaining.Count == 0 && !sawTransientExit) break;
          if (remaining.Count > 0 && !hasUnlisted && !sawTransientExit) {
            Console.Error.WriteLine("[e2e-job] releasing lingering build daemon(s): " + string.Join(", ", remaining.ToArray()));
            break;
          }
          if (!loggedSlowWait && wait.ElapsedMilliseconds >= WAIT_DIAGNOSTIC_MS) {
            loggedSlowWait = true;
            Console.Error.WriteLine("[e2e-job] waiting >10s after root exit for job process(es): " + string.Join(", ", remaining.ToArray()));
          }
          Thread.Sleep(20);
        }
      }

      // The wrapper itself is in the job. Keep the handle until process exit so the OS closes
      // it atomically and terminates any remaining processes in the job.
      GC.KeepAlive(job);
      return code;
    } catch (Exception ex) {
      Console.Error.WriteLine(ex.ToString());
      return 1;
    }
  }
}
`;

let windowsJobHelper: { dir?: NativePath; exe: NativePath } | undefined;
let treeSequence = 0;

function findWindowsCsc(): NativePath {
  const root = variable("SystemRoot") ?? "C:\\Windows";
  const candidates = [
    join(root, "Microsoft.NET", "Framework64", "v4.0.30319", "csc.exe"),
    join(root, "Microsoft.NET", "Framework", "v4.0.30319", "csc.exe"),
  ];
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) throw new Error(`cannot compile Windows Job Object helper: csc.exe not found at ${candidates.join(" or ")}`);
  return found as NativePath;
}

/** Prepare per-run process-tree resources once before build/batch steps (compiles the
 * standalone Windows Job Object helper once; no-op on other OSes). Returns a cleanup handle. */
export function prepareProcessTrees(): () => void {
  if (!WINDOWS) return () => {};
  const existing = variable("DESKTOP_TEST_JOB_HELPER");
  if (existing && existsSync(existing)) {
    windowsJobHelper = { exe: existing as NativePath };
    return () => {};
  }
  if (windowsJobHelper && existsSync(windowsJobHelper.exe)) return () => {};
  const dir = newDirectory(scratchDirectory(), "artifactize-e2e-job-");
  const source = nativePath(dir, "job.cs");
  const exe = nativePath(dir, "job.exe");
  try {
    writeText(source, WINDOWS_JOB_SOURCE);
    const csc = findWindowsCsc();
    const compiled = spawnSync(csc, ["/nologo", "/optimize+", "/platform:anycpu", "/r:System.dll", "/target:exe", `/out:${exe}`, source], {
      encoding: "utf8", windowsHide: true, timeout: 60000,
    });
    if (compiled.error || compiled.status !== 0 || !existsSync(exe)) {
      const detail = compiled.error ? String(compiled.error) : (compiled.stderr.trim() || compiled.stdout.trim() || `exited ${compiled.status}`);
      throw new Error(`failed to compile Windows Job Object helper with ${csc}: ${detail}`);
    }
    windowsJobHelper = { dir, exe };
    setVariable("DESKTOP_TEST_JOB_HELPER", exe);
    const dispose = (bestEffort: boolean) => {
      if (windowsJobHelper?.dir === dir) {
        windowsJobHelper = undefined;
        if (variable("DESKTOP_TEST_JOB_HELPER") === exe) setVariable("DESKTOP_TEST_JOB_HELPER", undefined);
        try { removeTree(dir); }
        catch (error) { if (!bestEffort) throw error; }
      }
    };
    const onExit = () => dispose(true);
    process.once("exit", onExit);
    return () => {
      process.off("exit", onExit);
      dispose(false);
    };
  } catch (error) {
    removeTree(dir);
    throw error;
  }
}

function ensureWindowsJobHelper(): NativePath {
  prepareProcessTrees();
  if (!windowsJobHelper || !existsSync(windowsJobHelper.exe)) {
    throw new Error("Windows Job Object helper executable is unavailable");
  }
  return windowsJobHelper.exe;
}

/** Windows' argv quoting for CreateProcess (including trailing backslashes). */
function windowsArg(arg: string): string {
  return `"${arg.replace(/(\\*)"/g, '$1$1\\"').replace(/\\+$/, '$&$&')}"`;
}

/** Whether a child spawned by a build step needs `detached: true` to outlive its launcher
 * while still remaining owned by `startTree`: on Windows libuv assigns non-detached children
 * to a launcher-scoped kill-on-close job (whereas detached children stay in our outer Job
 * Object); on Unix non-detached children stay in `startTree`'s process group. */
export const survivingChildDetached = WINDOWS;

/** Start a program with an owned process group, or a Windows Job Object. */
export function startTree(program: string, args: readonly string[], options: { cwd: NativePath; env: Environment; capture: boolean; captureDiagnostics?: boolean; completesWithRoot?: boolean }): ChildProcess {
  const stdio = ["ignore", options.capture ? "pipe" : "inherit", options.captureDiagnostics ? "pipe" : "inherit"] as const;
  if (WINDOWS) {
    const exe = ensureWindowsJobHelper();
    let env: Environment = {
      ...options.env,
      DESKTOP_BUILD_PROGRAM: program,
      DESKTOP_BUILD_ARGS: args.map(windowsArg).join(" "),
      DESKTOP_TREE_COMPLETES_WITH_ROOT: options.completesWithRoot ? "1" : "0",
    };
    if (!options.completesWithRoot && !hasVariable(env, "_MSPDBSRV_ENDPOINT_")) {
      env = withVariable(env, "_MSPDBSRV_ENDPOINT_", `artifactize-e2e-${process.pid}-${++treeSequence}-${randomBytes(4).toString("hex")}`);
    }
    return spawn(exe, [], { cwd: options.cwd, env, windowsHide: true, stdio: [...stdio] });
  }
  return spawn(program, args, { cwd: options.cwd, env: options.env, detached: true, windowsHide: true, stdio: [...stdio] });
}

/** A batch needs the same Windows kill-on-close ownership as the build tree. On Unix the
 * terminal's signal reaches the batch and WDIO runs its teardown hooks. */
export function startBatch(program: string, args: readonly string[], env: Environment, cwd: NativePath): ChildProcess {
  return WINDOWS ? startTree(program, args, { cwd, env, capture: false, completesWithRoot: true }) : spawn(program, args, { env, cwd, stdio: "inherit" });
}

/** End the owned tree, not just the command whose exit status the wrapper reports. */
export function stopTree(child: ChildProcess): void {
  if (child.pid === undefined) return;
  if (WINDOWS) {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    return;
  }
  try { process.kill(-child.pid, "SIGKILL"); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
}

export type ProcessState = "running" | "exited" | "unknown";
export const isPid = (pid: unknown): pid is number => typeof pid === "number" && Number.isSafeInteger(pid) && pid > 0 && pid <= 0xffffffff;

/** Observe this native PID without signalling it. On Windows the zero-signal libuv probe
 * opens a process handle; on Unix it checks the PID. Permission or read errors aren't exits. */
export function processState(pid: number): ProcessState {
  if (!isPid(pid)) return "unknown";
  try { process.kill(pid, 0); }
  catch (error) {
    return (error as NodeJS.ErrnoException).code === "ESRCH" ? "exited" : "unknown";
  }
  if (!LINUX) return "running";
  try {
    // A dead orphan may retain its PID when the container's init does not reap it.
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const close = stat.lastIndexOf(")");
    const state = close < 0 ? "" : stat.slice(close + 2, close + 3);
    return !state ? "unknown" : "ZXx".includes(state) ? "exited" : "running";
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "ENOENT" ? "exited" : "unknown";
  }
}

/** For cleanup gates, an inconclusive observation must be treated as still owned. */
export function processRunning(pid: number): boolean {
  return processState(pid) !== "exited";
}

/** Only positive native exit evidence can complete a Quit or restart. */
export async function waitForProcessExit(pid: number, options: { timeout?: number; observe?: (pid: number) => ProcessState } = {}): Promise<void> {
  if (!isPid(pid)) throw new Error("the app did not provide an owned native process ID");
  const { timeout = 30000, observe = processState } = options;
  const deadline = performance.now() + timeout;
  for (;;) {
    const state = observe(pid);
    if (state === "exited") return;
    const left = deadline - performance.now();
    if (left <= 0) throw new Error(`the app's exit was not confirmed for PID ${pid}: ${state}`);
    await delay(Math.min(100, left));
  }
}

/** Wait until any Linux child process that inherited this fixture's isolated HOME (such as
 * WebKitGTK's WebKitNetworkProcess or WebKitWebProcess flushing storage after the app stops)
 * has exited, so it cannot recreate directories inside `root` after teardown deletes them.
 * Orphans that hang after the app has already stopped are escalated with SIGTERM then SIGKILL. */
export async function waitForFixtureProcesses(root: NativePath, timeout = 10000): Promise<void> {
  if (!LINUX) return;
  const home = `HOME=${nativePath(root, "home")}\0`;
  const startMarker = Buffer.from(home);
  const midMarker = Buffer.from(`\0${home}`);
  const alive = (): { pid: number; label: string }[] => {
    const found: { pid: number; label: string }[] = [];
    for (const entry of readdirSync("/proc")) {
      if (!/^\d+$/.test(entry)) continue;
      const pid = Number(entry);
      if (pid === process.pid || processState(pid) === "exited") continue;
      try {
        const env = readFileSync(`/proc/${pid}/environ`);
        if (!env.subarray(0, startMarker.length).equals(startMarker) && !env.includes(midMarker)) continue;
      } catch { continue; }
      let name = "";
      try { name = readFileSync(`/proc/${pid}/comm`, "utf8").trim(); } catch { /* exited while reading */ }
      found.push({ pid, label: name ? `${pid} (${name})` : String(pid) });
    }
    return found;
  };
  const wait = async (ms: number): Promise<{ pid: number; label: string }[]> => {
    const end = performance.now() + ms;
    for (;;) {
      const remaining = alive();
      if (!remaining.length) return [];
      const left = end - performance.now();
      if (left <= 0) return remaining;
      await delay(Math.min(50, left));
    }
  };
  const signal = (processes: { pid: number }[], sig: NodeJS.Signals) => {
    for (const { pid } of processes) {
      try { process.kill(pid, sig); }
      catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
    }
  };
  const deadline = performance.now() + timeout;
  let remaining = await wait(Math.min(2000, timeout));
  if (remaining.length) {
    signal(remaining, "SIGTERM");
    remaining = await wait(Math.min(2000, Math.max(0, deadline - performance.now())));
  }
  if (remaining.length) {
    signal(remaining, "SIGKILL");
    remaining = await wait(Math.max(0, deadline - performance.now()));
  }
  if (remaining.length) {
    throw new Error(`fixture processes still running for ${root}: ${remaining.map((each) => each.label).join(", ")}`);
  }
}

/** The variables that send a build's temporary files into `folder`, as this OS's tools read
 * them. */
export function tempEnvironment(folder: NativePath): Environment {
  return WINDOWS ? { TMP: folder, TEMP: folder } : { TMPDIR: folder };
}

// ---------------------------------------------------------------------------------
// The app's session

/** The runner command `args` (a Node script and its arguments) as this OS runs it headless:
 * on Linux inside its own D-Bus session and X server, so no app reaches the user's desktop. */
export function runnerCommand(args: string[], env: Environment = process.env): { program: string; args: string[]; env: Environment } {
  if (!LINUX) return { program: process.execPath, args, env };
  // The batch checks that it got a display and a bus of its own, not these (assertHeadless).
  const outer = { DESKTOP_TEST_OUTER_DISPLAY: env.DISPLAY ?? "", DESKTOP_TEST_OUTER_BUS: env.DBUS_SESSION_BUS_ADDRESS ?? "" };
  const headless: Environment = { ...env, ...outer, GDK_BACKEND: "x11" };
  delete headless.WAYLAND_DISPLAY;
  const framebuffer = env.DESKTOP_TEST_BATCH_OUTPUT ? ["-s", "-screen 0 1280x1024x24 -fbdir ."] : [];
  return { program: "dbus-run-session", args: ["--", "xvfb-run", "-a", ...framebuffer, process.execPath, ...args], env: headless };
}

/** Use a native capture tool only for failure evidence, never for test input or assertions. */
function capture(program: string, args: string[]): string {
  const result = spawnSync(program, args, { encoding: "utf8", windowsHide: true, timeout: 20000 });
  if (result.error || result.status !== 0) throw new Error(`${program} could not capture the owned window: ${result.error ?? (result.stderr.trim() || result.status)}`);
  return result.stdout.trim();
}

const MAC_WINDOW = `
ObjC.import('CoreGraphics');
function run(args) {
  const pid = Number(args[0]);
  const windows = ObjC.deepUnwrap($.CGWindowListCopyWindowInfo(17, 0));
  const window = windows.find(w => w.kCGWindowOwnerPID === pid && w.kCGWindowLayer === 0 && w.kCGWindowBounds.Width > 0 && w.kCGWindowBounds.Height > 0);
  if (!window) throw new Error('no visible window for the owned app');
  return String(window.kCGWindowNumber);
}`;

const WINDOWS_WINDOW = `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class E2eWindowCapture {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect bounds);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
}
'@
[void][E2eWindowCapture]::SetProcessDPIAware()
$owner = Get-Process -Id $env:DESKTOP_EVIDENCE_PID
$window = $owner.MainWindowHandle
$bounds = New-Object E2eWindowCapture+Rect
$windowOwner = [uint32]0
if ($window -eq 0 -or -not [E2eWindowCapture]::GetWindowRect($window, [ref]$bounds)) { throw 'no visible window for the owned app' }
[void][E2eWindowCapture]::GetWindowThreadProcessId($window, [ref]$windowOwner)
if ($windowOwner -ne $owner.Id -or $owner.HasExited) { throw 'the owned app no longer holds the window' }
$width = $bounds.Right - $bounds.Left
$height = $bounds.Bottom - $bounds.Top
$image = New-Object System.Drawing.Bitmap -ArgumentList $width, $height
try {
  $graphics = [System.Drawing.Graphics]::FromImage($image)
  try {
    $dc = $graphics.GetHdc()
    try { if (-not [E2eWindowCapture]::PrintWindow($window, $dc, 2)) { throw 'PrintWindow failed' } }
    finally { $graphics.ReleaseHdc($dc) }
  } finally { $graphics.Dispose() }
  $image.Save($env:DESKTOP_EVIDENCE_IMAGE, [System.Drawing.Imaging.ImageFormat]::Png)
} finally { $image.Dispose() }
`;

/** New Session can fail after the window opens but before WebDriver can capture it. The private
 * Xvfb screen is available directly; other systems capture only the service-owned app window. */
export function displayScreenshot(output: NativePath, pid?: number): Uint8Array {
  if (LINUX) return xwdPng(readFileSync(nativePath(output, "Xvfb_screen0")));
  if (!pid) throw new Error("there is no live service-owned app to capture");
  const scratch = newDirectory(output, "window-evidence-");
  const image = nativePath(scratch, "window.png");
  try {
    if (MACOS) {
      const id = capture("/usr/bin/osascript", ["-l", "JavaScript", "-e", MAC_WINDOW, String(pid)]);
      if (!/^\d+$/.test(id)) throw new Error("the owned window ID was not returned");
      capture("/usr/sbin/screencapture", ["-x", `-l${id}`, image]);
    } else {
      // An encoded, fixed script and environment values avoid shell parsing of the path.
      const powershell = join(variable("SystemRoot") ?? "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
      const script = Buffer.from(WINDOWS_WINDOW, "utf16le").toString("base64");
      const env = { ...process.env, DESKTOP_EVIDENCE_PID: String(pid), DESKTOP_EVIDENCE_IMAGE: image };
      const result = spawnSync(powershell, ["-NoProfile", "-NonInteractive", "-EncodedCommand", script], { env, windowsHide: true, timeout: 20000, encoding: "utf8" });
      if (result.error || result.status !== 0) throw new Error(`could not capture the owned window: ${result.error ?? (result.stderr.trim() || result.status)}`);
    }
    return readFileSync(image);
  } finally { removeTree(scratch); }
}

/** The app's own WebKit image is independent of OS screen-capture permission. */
export function fallbackImageEnvironment(path: NativePath): Environment {
  return MACOS ? { ARTIFACTIZE_E2E_WINDOW_IMAGE: path } : {};
}

export function fallbackImage(path?: NativePath): Uint8Array | undefined {
  if (!path || !MACOS) return undefined;
  try { return readFileSync(path); } catch { return undefined; }
}

/** The service spawns the app directly under the batch. Before the driver is ready, the
 * configured state path distinguishes this fixture from all other children and apps. This
 * lookup is for read-only screenshots, never for signalling a process. */
function startupPid(state: NativePath): number | undefined {
  if (MACOS) {
    const listed = spawnSync("/bin/ps", ["-ww", "-axo", "pid=,ppid=,command="], { encoding: "utf8", timeout: 1000 });
    for (const line of listed.stdout?.split("\n") ?? []) {
      const parsed = /^\s*(\d+)\s+(\d+)\s+(.*)/.exec(line);
      if (parsed && Number(parsed[2]) === process.pid && parsed[3].includes(`--state-dir ${state} --reviewer`)) return Number(parsed[1]);
    }
    return undefined;
  }
  const powershell = join(variable("SystemRoot") ?? "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
  const query = `Get-CimInstance Win32_Process -Filter ('ParentProcessId=' + $env:DESKTOP_STARTUP_PARENT) | Where-Object { $_.CommandLine -and $_.CommandLine.Contains($env:DESKTOP_STARTUP_STATE) } | Select-Object -First 1 -ExpandProperty ProcessId`;
  const listed = spawnSync(powershell, ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(query, "utf16le").toString("base64")], {
    env: { ...process.env, DESKTOP_STARTUP_PARENT: String(process.pid), DESKTOP_STARTUP_STATE: state },
    encoding: "utf8", windowsHide: true, timeout: 3000,
  });
  const text = listed.stdout?.trim() ?? "";
  return /^\d+$/.test(text) ? Number(text) : undefined;
}

/** Retain the last startup frame before the service kills its timed-out process. */
export function startupImage(output: NativePath, state: NativePath): (encode?: boolean) => Uint8Array | undefined {
  if (!LINUX) {
    let last: Uint8Array | undefined;
    const timer = setInterval(() => {
      const pid = startupPid(state);
      if (!pid) return;
      try { last = displayScreenshot(output, pid); } catch { /* no native window yet */ }
    }, 5000);
    timer.unref();
    return (encode = true) => { clearInterval(timer); return encode ? last : undefined; };
  }
  const frame = nativePath(output, "Xvfb_screen0");
  const baseline = readFileSync(frame);
  let last: Buffer | undefined;
  const sample = () => {
    try {
      const pixels = readFileSync(frame);
      if (!pixels.equals(baseline)) last = pixels;
    } catch { /* the framebuffer may disappear as the display ends */ }
  };
  const timer = setInterval(sample, 250);
  timer.unref();
  return (encode = true) => {
    clearInterval(timer);
    return encode && last ? xwdPng(last) : undefined;
  };
}

/** Decode the Xvfb screen's ZPixmap into a portable evidence PNG. */
export function xwdPng(frame: Buffer): Uint8Array {
  if (frame.length < 100 || frame.readUInt32BE(4) !== 7 || frame.readUInt32BE(8) !== 2 || frame.readUInt32BE(44) !== 32) {
    throw new Error("the isolated display did not write a 32-bit ZPixmap XWD frame");
  }
  const width = frame.readUInt32BE(16), height = frame.readUInt32BE(20);
  const stride = frame.readUInt32BE(48);
  const start = frame.readUInt32BE(0) + frame.readUInt32BE(76) * 12;
  if (!width || !height || width * 4 > stride || start + stride * height > frame.length) throw new Error("the isolated display's XWD frame is truncated");
  const littleEndian = frame.readUInt32BE(28) === 0;
  const masks = [56, 60, 64].map((offset) => frame.readUInt32BE(offset));
  const shifts = masks.map((mask) => 31 - Math.clz32(mask & -mask));
  const image = new PNG({ width, height });
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const offset = start + y * stride + x * 4;
    const pixel = littleEndian ? frame.readUInt32LE(offset) : frame.readUInt32BE(offset);
    const dest = (y * width + x) * 4;
    for (let c = 0; c < 3; c++) image.data[dest + c] = Math.round(255 * ((pixel & masks[c]) >>> shifts[c]) / (masks[c] >>> shifts[c]));
    image.data[dest + 3] = 255;
  }
  return PNG.sync.write(image);
}

/** Fail unless this process runs where `runnerCommand` puts it: on Linux, a display and a
 * session bus of its own, never the ones it was started from nor a Wayland desktop. */
export function assertHeadless(batchOutput?: NativePath): void {
  const { DISPLAY: display, DBUS_SESSION_BUS_ADDRESS: bus, DESKTOP_TEST_OUTER_DISPLAY: outerDisplay, DESKTOP_TEST_OUTER_BUS: outerBus } = process.env;
  if (LINUX && (!display || !bus || outerDisplay === undefined || outerBus === undefined || display === outerDisplay || bus === outerBus || process.env.WAYLAND_DISPLAY)) {
    throw new Error("Linux desktop E2E needs its own D-Bus session and Xvfb display: start it with `pnpm e2e`");
  }
  if (LINUX && batchOutput !== undefined && !existsSync(nativePath(batchOutput, "Xvfb_screen0"))) {
    throw new Error(`Linux desktop E2E batch needs a private framebuffer at ${nativePath(batchOutput, "Xvfb_screen0")}. Run pnpm e2e, or give the batch's Xvfb -screen 0 1280x1024x24 -fbdir with its output directory.`);
  }
}

/** The display and session variables an app needs on this OS, kept through fixture isolation;
 * on Linux GTK's input method is fixed for the run instead of the host's. */
export function displayEnvironment(): Environment {
  if (!LINUX) return {};
  const env: Environment = { GDK_BACKEND: "x11", GTK_IM_MODULE: "gtk-im-context-simple", XMODIFIERS: "", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" };
  for (const name of ["DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS"]) if (process.env[name] !== undefined) env[name] = process.env[name];
  return env;
}

/** The test app's window options: macOS would stop drawing a window another app covers. */
export function testWindowOptions(windows: readonly Record<string, unknown>[]): Record<string, unknown> {
  return MACOS ? { windows: windows.map((window) => ({ ...window, backgroundThrottling: "disabled" })) } : {};
}

/** WebView2 gives focus events only to the foreground window. A private Xvfb screen belongs
 * to one scenario, so its failure evidence cannot capture another scenario's window. */
export const appsAtOnce = WINDOWS || LINUX ? 1 : 2;

/** This machine's OS as a failure names it. */
export const systemName = `${process.platform} ${release()} (${process.arch})`;

/** Every batch on a shared desktop takes the same lock, including ordinary batches that
 * could otherwise interfere with another suite's exclusive scenario. Isolated Xvfb displays
 * need no desktop lock; the runner still locks a scenario's other shared resources. */
export const desktopResources: readonly string[] = WINDOWS || MACOS ? ["foreground"] : [];

/** The foreground lock's fail-closed marker survives a batch process that must exit without
 * having confirmed it stopped its app. The path is shared by suite runs in this desktop's
 * user account, independent of the per-run scratch, state and sanitized environment. */
export function foregroundQuarantine(): NativePath | null {
  return WINDOWS || MACOS ? nativePath(homedir(), ".artifactize-desktop-e2e", "foreground-unreleased") : null;
}

type ForegroundRecord = { description: string; pids: number[]; jobOwned: boolean };

/** Until the service has published its child handles, the Windows job wrapper itself owns
 * all of its descendants. A direct invocation must not claim this recovery guarantee. */
export function batchJobPids(): number[] {
  const wrapper = Number(variable("DESKTOP_TEST_BATCH_JOB_PID"));
  return WINDOWS && isPid(wrapper) && wrapper === process.ppid ? [process.pid, wrapper] : [];
}

/** Check under the foreground lock. A stale Windows record is recoverable only when the
 * job's owning wrapper, batch and every recorded app PID are all confirmed gone. */
export function checkForeground(path: NativePath | null, observe = processState): void {
  if (!path) return;
  const saved = readTextIfExists(path);
  if (saved === null) return;
  let record: Partial<ForegroundRecord> | null = null;
  try { record = JSON.parse(saved) as Partial<ForegroundRecord> | null; } catch { /* legacy marker; fail closed */ }
  const pids = Array.isArray(record?.pids) && record.pids.every(isPid) ? record.pids : [];
  if (record?.jobOwned === true && new Set(pids).size >= 2 && pids.every((pid) => observe(pid) === "exited")) {
    releaseForeground(path);
    return;
  }
  const owners = pids.length ? ` Owned PIDs: ${pids.join(", ")}.` : "";
  throw new Error(`the desktop foreground is quarantined: ${record?.description ?? saved.trim()}${owners} Confirm the orphaned fixture app has exited before removing ${path}.`);
}

/** Keep the marker replaceable as the launcher publishes additional app PIDs. A killed
 * writer leaves the last complete record, which already contains the Windows job owner. */
export function quarantineForeground(path: NativePath | null, description: string, pids: readonly number[] = [], jobOwned = false): void {
  if (!path) return;
  makeDirectory(nativePath(path, ".."), { private: true });
  replaceText(path, JSON.stringify({ description, pids: [...new Set(pids)], jobOwned } satisfies ForegroundRecord));
}

/** Only once every app is stopped, clear the persistent half of the foreground lock. */
export function releaseForeground(path: NativePath | null): void {
  if (path) rmSync(path, { force: true });
}

/** On Windows, give the scenario's window the desktop's focus before it starts (see
 * appsAtOnce); other systems deliver focus events to a page in any window. */
export async function takeSystemFocus(browser: WebdriverIO.Browser): Promise<void> {
  if (!WINDOWS) return;
  await callWindow(browser, "setFocus");
  await browser.waitUntil(() => browser.execute(() => document.hasFocus()),
    { timeout: 15000, timeoutMsg: "setup failed: the app window did not take the system focus" });
}

/** Where the app finds the system browser's opener, made to run `client` (a Node program given
 * the address) instead of the person's browser: on Unix the opener command first on its PATH,
 * on Windows the test build's opener seam, which stands where the default association would. */
export function installBrowserOpener(root: NativePath, env: Environment, client: NativePath): Environment {
  const browser = { DESKTOP_TEST_NODE: nodeProgram, DESKTOP_TEST_BROWSER_CLIENT: client };
  if (WINDOWS) return { ...env, ...browser, ARTIFACTIZE_E2E_BROWSER_NODE: nodeProgram, ARTIFACTIZE_E2E_BROWSER_PROGRAM: client };
  const bin = makeDirectory(nativePath(root, "browser-bin"));
  writeNodeCommand(bin, MACOS ? "open" : "xdg-open", "DESKTOP_TEST_BROWSER_CLIENT");
  return { ...withPathFirst(env, [bin]), ...browser };
}
