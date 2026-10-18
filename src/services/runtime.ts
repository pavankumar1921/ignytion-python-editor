// Thin typed wrappers over the native commands. The frontend never decides
// which Python to use; it only displays what the native layer verified.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export interface RuntimeStatus {
  verified: boolean;
  pythonPath: string | null;
  version: string | null;
  packageDir: string | null;
  source: "marker" | "user-selected" | "dev-override" | string;
  message: string;
  checks: Check[];
}

export interface RunOutput {
  stream: "stdout" | "stderr";
  text: string;
}

export interface RunResult {
  exitCode: number | null;
  killed: boolean;
  durationMs: number;
  error: string | null;
}

export const detectRuntime = () => invoke<RuntimeStatus>("get_runtime_status");
export const selectInterpreter = (path: string) =>
  invoke<RuntimeStatus>("select_interpreter", { path });
export const runPython = (scriptPath: string) =>
  invoke<RunResult>("run_python", { scriptPath });
export const stopPython = () => invoke<boolean>("stop_python");
export const onRunOutput = (cb: (o: RunOutput) => void): Promise<UnlistenFn> =>
  listen<RunOutput>("run-output", (e) => cb(e.payload));
