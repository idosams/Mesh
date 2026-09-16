// protocol/conformance/lib/adapter.mjs — the process boundary the suite runs across
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// The suite never imports a client. It spawns one and speaks `cwp-conformance-adapter/0` to it:
// one JSON object per line in, one JSON object per line out, in order. That boundary is what makes
// the result mean something — a suite that calls a function is testing a build, and a suite that
// crosses a process boundary with a documented byte protocol is testing an implementation.
//
// It is not a network socket, and the README says so plainly rather than claiming more than is
// true: no CWP transport exists yet, so there is no session to run over. The boundary this suite
// has is the strongest one available today, and the adapter contract is written so the same client
// answers unchanged when a transport arrives.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const DEFAULT_TIMEOUT_MS = 15_000;

export class AdapterError extends Error {}

export class Adapter {
  #child;
  #pending = [];
  #closed = null;
  #stderr = "";

  constructor(command, args, { cwd, env, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    this.command = [command, ...args].join(" ");
    this.timeoutMs = timeoutMs;
    this.#child = spawn(command, args, {
      cwd,
      env: { ...process.env, ...env },
      stdio: ["pipe", "pipe", "pipe"],
    });

    createInterface({ input: this.#child.stdout, crlfDelay: Infinity }).on("line", (line) => {
      if (line.trim() === "") return;
      const waiter = this.#pending.shift();
      if (!waiter) return;
      try {
        waiter.resolve(JSON.parse(line));
      } catch {
        waiter.reject(new AdapterError(`the adapter wrote a line that is not JSON: ${line}`));
      }
    });

    this.#child.stderr.on("data", (chunk) => {
      this.#stderr += String(chunk);
      if (this.#stderr.length > 4096) this.#stderr = this.#stderr.slice(-4096);
    });

    const close = (reason) => {
      this.#closed = reason;
      while (this.#pending.length > 0) this.#pending.shift().reject(new AdapterError(reason));
    };
    this.#child.on("error", (error) => close(`the adapter could not be started: ${error.message}`));
    this.#child.on("exit", (code, signal) => {
      const how = signal ? `signal ${signal}` : `exit code ${code}`;
      close(`the adapter exited (${how})${this.#stderr ? `: ${this.#stderr.trim()}` : ""}`);
    });
  }

  /** One request, one response. Rejects with `AdapterError` when the adapter misbehaves. */
  async ask(request) {
    if (this.#closed) throw new AdapterError(this.#closed);
    const response = new Promise((resolve, reject) => {
      this.#pending.push({ resolve, reject });
      this.#child.stdin.write(`${JSON.stringify(request)}\n`, (error) => {
        if (error) reject(new AdapterError(`writing to the adapter failed: ${error.message}`));
      });
    });

    let timer;
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(
        () => reject(new AdapterError(`the adapter did not answer ${request.op} within ${this.timeoutMs} ms`)),
        this.timeoutMs,
      );
    });

    try {
      const answer = await Promise.race([response, timeout]);
      if (answer === null || typeof answer !== "object") {
        throw new AdapterError(`the adapter answered ${request.op} with something that is not an object`);
      }
      return answer;
    } finally {
      clearTimeout(timer);
    }
  }

  close() {
    this.#child.stdin.end();
    this.#child.kill();
  }
}

/**
 * Open an adapter and complete the `hello` exchange, which is the only request whose shape the
 * suite fixes for every client: it is where a client declares what it can be asked about.
 */
export async function open(command, args, options) {
  const adapter = new Adapter(command, args, options);
  const hello = await adapter.ask({ op: "hello" });
  if (hello.ok !== true) {
    adapter.close();
    throw new AdapterError(`the adapter refused hello: ${hello.reason ?? JSON.stringify(hello)}`);
  }
  if (hello.adapter !== "cwp-conformance-adapter/0") {
    adapter.close();
    throw new AdapterError(
      `the adapter answered hello with adapter ${JSON.stringify(hello.adapter)}, ` +
        "expected cwp-conformance-adapter/0",
    );
  }
  if (!Array.isArray(hello.capabilities)) {
    adapter.close();
    throw new AdapterError("the adapter answered hello without a capabilities array");
  }
  return { adapter, hello };
}
