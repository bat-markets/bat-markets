import { existsSync } from "node:fs";
import { resolve } from "node:path";

type FieldType = "text" | "number" | "select" | "boolean";
type RunStatus = "starting" | "running" | "finished" | "failed" | "stopped";
type LogSource = "stdout" | "stderr" | "system";

type ScenarioField = {
  key: string;
  label: string;
  type: FieldType;
  description: string;
  defaultValue: string;
  options?: Array<{ label: string; value: string }>;
};

type Scenario = {
  id: string;
  title: string;
  category: string;
  description: string;
  risk: "fixture" | "read" | "write";
  requiresCredentials: boolean;
  example: string;
  fields: ScenarioField[];
};

type RunLog = {
  at: string;
  source: LogSource;
  text: string;
};

type RunRecord = {
  id: string;
  scenarioId: string;
  title: string;
  example: string;
  status: RunStatus;
  startedAt: string;
  endedAt: string | null;
  pid: number | null;
  exitCode: number | null;
  logs: RunLog[];
  env: Record<string, string>;
  process: Bun.Subprocess | null;
  listeners: Set<ReadableStreamDefaultController<string>>;
};

const PORT = Number(process.env.BAT_MARKETS_WEB_PORT || 3107);
const HOST = process.env.BAT_MARKETS_WEB_HOST || "127.0.0.1";
const REPO_ROOT = resolve(import.meta.dir, "..", "..");
const INDEX_HTML = Bun.file(resolve(import.meta.dir, "index.html"));
const DEFAULT_CREDENTIALS_PATH =
  process.env.BAT_MARKETS_CREDENTIALS_FILE ||
  process.env.EXCHANGE_API_EXPERT_CREDENTIALS_FILE ||
  "/Users/kirillovdigital/.codex/skills/exchange-api-expert/credentials.env";
const LOADED_CREDENTIALS = await loadCredentials(DEFAULT_CREDENTIALS_PATH);
const MAX_LOG_LINES = 1500;

const SCENARIOS: Scenario[] = [
  {
    id: "live-realtime-monitor",
    title: "Live Realtime Monitor",
    category: "Live Monitor",
    description:
      "Continuous public or private stream monitor for one symbol. Best quick panel for watching how events land in realtime.",
    risk: "read",
    requiresCredentials: false,
    example: "live_realtime_monitor",
    fields: [
      selectField("BAT_MARKETS_VENUE", "Venue", "binance", "Exchange venue.", [
        { label: "Binance", value: "binance" },
        { label: "Bybit", value: "bybit" },
      ]),
      textField(
        "BAT_MARKETS_SYMBOL",
        "Symbol",
        "BTC/USDT:USDT",
        "Unified instrument id used by the example."
      ),
      numberField("BAT_MARKETS_MAX_EVENTS", "Max Events", "40", "How many events to print before stopping."),
      numberField("BAT_MARKETS_MAX_SECONDS", "Max Seconds", "60", "Hard timeout for the monitor run."),
      boolField("BAT_MARKETS_WATCH_TICKER", "Ticker", "1", "Watch unified ticker updates."),
      boolField("BAT_MARKETS_WATCH_TRADES", "Trades", "1", "Watch unified trade ticks."),
      boolField("BAT_MARKETS_WATCH_BOOK_TOP", "Book Top", "1", "Watch top-of-book updates."),
      boolField("BAT_MARKETS_WATCH_MARK_PRICE", "Mark Price", "1", "Watch mark price updates."),
      boolField("BAT_MARKETS_WATCH_ORDERS", "Orders", "0", "Watch private order updates. Requires creds."),
      boolField("BAT_MARKETS_WATCH_EXECUTIONS", "Executions", "0", "Watch private executions. Requires creds."),
      boolField("BAT_MARKETS_WATCH_POSITIONS", "Positions", "0", "Watch private positions. Requires creds."),
      boolField("BAT_MARKETS_WATCH_BALANCES", "Balances", "0", "Watch private balances. Requires creds."),
      boolField("BAT_MARKETS_WATCH_ACCOUNT", "Account", "0", "Watch private account summary. Requires creds."),
    ],
  },
  {
    id: "live-public-multiwatch",
    title: "Live Public Multiwatch",
    category: "Live Public",
    description:
      "Short live public probe for ticker, mark price, and liquidation on one symbol.",
    risk: "read",
    requiresCredentials: false,
    example: "live_public_multiwatch",
    fields: [
      selectField("BAT_MARKETS_VENUE", "Venue", "binance", "Exchange venue.", [
        { label: "Binance", value: "binance" },
        { label: "Bybit", value: "bybit" },
      ]),
      textField(
        "BAT_MARKETS_SYMBOL",
        "Symbol",
        "BTC/USDT:USDT",
        "Unified instrument id used by the example."
      ),
    ],
  },
  {
    id: "live-trade-probe",
    title: "Live Trade Probe",
    category: "Live Public",
    description:
      "Short live public probe for raw event flow, trade ticks, and book-top confirmation.",
    risk: "read",
    requiresCredentials: false,
    example: "live_trade_probe",
    fields: [
      selectField("BAT_MARKETS_VENUE", "Venue", "binance", "Exchange venue.", [
        { label: "Binance", value: "binance" },
        { label: "Bybit", value: "bybit" },
      ]),
      textField(
        "BAT_MARKETS_SYMBOL",
        "Symbol",
        "BTC/USDT:USDT",
        "Unified instrument id used by the example."
      ),
    ],
  },
  {
    id: "live-entry-validate",
    title: "Live Entry Validate",
    category: "Live Private",
    description:
      "Venue-native dry-run validation for a limit order. Safe but requires live private credentials.",
    risk: "read",
    requiresCredentials: true,
    example: "live_entry_validate",
    fields: [
      selectField("BAT_MARKETS_VENUE", "Venue", "binance", "Exchange venue.", [
        { label: "Binance", value: "binance" },
        { label: "Bybit", value: "bybit" },
      ]),
      textField(
        "BAT_MARKETS_SYMBOL",
        "Symbol",
        "BTC/USDT:USDT",
        "Unified instrument id used by the validation example."
      ),
    ],
  },
  {
    id: "binance-trade-cycle",
    title: "Binance Trade Cycle",
    category: "Live Trading",
    description:
      "Real Binance USDⓈ-M cycle: market open, market close, maker buy/sell place and cancel. Places live orders.",
    risk: "write",
    requiresCredentials: true,
    example: "live_binance_trade_cycle",
    fields: [],
  },
  {
    id: "binance-extended-stress",
    title: "Binance Extended Stress",
    category: "Live Trading",
    description:
      "Real Binance stress run with protective TP/SL, burst create/cancel rounds, and latency output. Places live orders.",
    risk: "write",
    requiresCredentials: true,
    example: "live_binance_extended_stress",
    fields: [
      numberField(
        "BAT_MARKETS_BINANCE_EXTENDED_STRESS_ROUNDS",
        "Rounds",
        "3",
        "How many burst rounds to run."
      ),
      numberField(
        "BAT_MARKETS_BINANCE_EXTENDED_STRESS_BURST_SIZE",
        "Burst Size",
        "5",
        "How many orders per burst round."
      ),
    ],
  },
  {
    id: "runtime-batch-stub",
    title: "Runtime Batch Stub",
    category: "Fixture",
    description:
      "Local stubbed runtime path for validate, batch create, and batch cancel. No credentials required.",
    risk: "fixture",
    requiresCredentials: false,
    example: "runtime_batch_stub_probe",
    fields: [],
  },
  {
    id: "diagnostics-fixture",
    title: "Diagnostics Fixture Probe",
    category: "Fixture",
    description:
      "Fixture-driven diagnostics snapshot probe for state reads, writes, and operation counters.",
    risk: "fixture",
    requiresCredentials: false,
    example: "diagnostics_fixture_probe",
    fields: [],
  },
  {
    id: "private-stream-fixture",
    title: "Private Stream Fixture Probe",
    category: "Fixture",
    description:
      "Fixture-driven private stream probe for balances, positions, orders, executions, and health.",
    risk: "fixture",
    requiresCredentials: false,
    example: "private_stream_fixture_probe",
    fields: [],
  },
];

const runs = new Map<string, RunRecord>();

Bun.serve({
  hostname: HOST,
  port: PORT,
  async fetch(request) {
    const url = new URL(request.url);

    if (request.method === "GET" && (url.pathname === "/" || url.pathname === "/index.html")) {
      return new Response(INDEX_HTML, {
        headers: { "content-type": "text/html; charset=utf-8" },
      });
    }

    if (request.method === "GET" && url.pathname === "/api/health") {
      return json({
        ok: true,
        host: HOST,
        port: PORT,
        bunVersion: Bun.version,
        repoRoot: REPO_ROOT,
        credentialsPath: existsSync(DEFAULT_CREDENTIALS_PATH) ? DEFAULT_CREDENTIALS_PATH : null,
        credentialsLoaded: Object.keys(LOADED_CREDENTIALS).length > 0,
        activeRuns: [...runs.values()].filter((run) => run.status === "starting" || run.status === "running").length,
      });
    }

    if (request.method === "GET" && url.pathname === "/api/scenarios") {
      return json(
        SCENARIOS.map((scenario) => ({
          ...scenario,
          commandPreview: `cargo run --color never -p bat-markets --example ${scenario.example}`,
        }))
      );
    }

    if (request.method === "GET" && url.pathname === "/api/runs") {
      return json([...runs.values()].map(publicRun));
    }

    if (request.method === "POST" && url.pathname === "/api/runs") {
      const body = await request.json().catch(() => ({}));
      const scenario = SCENARIOS.find((item) => item.id === body?.scenarioId);
      if (!scenario) {
        return json({ error: "unknown scenario" }, 404);
      }

      const env = buildRunEnv(scenario, body?.env ?? {});
      const run = startRun(scenario, env);
      return json(publicRun(run), 201);
    }

    const stopMatch = url.pathname.match(/^\/api\/runs\/([^/]+)\/stop$/);
    if (request.method === "POST" && stopMatch) {
      const run = runs.get(stopMatch[1]);
      if (!run) {
        return json({ error: "run not found" }, 404);
      }
      stopRun(run);
      return json(publicRun(run));
    }

    const eventsMatch = url.pathname.match(/^\/api\/runs\/([^/]+)\/events$/);
    if (request.method === "GET" && eventsMatch) {
      const run = runs.get(eventsMatch[1]);
      if (!run) {
        return json({ error: "run not found" }, 404);
      }
      return sse(run);
    }

    return json({ error: "not found" }, 404);
  },
});

console.log(`bat-markets realtime web demo listening on http://${HOST}:${PORT}`);

function startRun(scenario: Scenario, env: Record<string, string>): RunRecord {
  const id = crypto.randomUUID();
  const run: RunRecord = {
    id,
    scenarioId: scenario.id,
    title: scenario.title,
    example: scenario.example,
    status: "starting",
    startedAt: new Date().toISOString(),
    endedAt: null,
    pid: null,
    exitCode: null,
    logs: [],
    env,
    process: null,
    listeners: new Set(),
  };

  runs.set(id, run);
  appendLog(run, "system", `starting scenario="${scenario.title}" example=${scenario.example}`);

  const child = Bun.spawn(["cargo", "run", "--color", "never", "-p", "bat-markets", "--example", scenario.example], {
    cwd: REPO_ROOT,
    env: {
      ...Object.fromEntries(
        Object.entries(process.env).map(([key, value]) => [key, value ?? ""])
      ),
      ...LOADED_CREDENTIALS,
      ...env,
    },
    stdout: "pipe",
    stderr: "pipe",
    stdin: "ignore",
  });

  run.process = child;
  run.pid = child.pid;
  run.status = "running";
  emit(run, "status", publicRun(run));
  appendLog(run, "system", `spawned pid=${child.pid}`);

  void pumpStream(run, child.stdout, "stdout");
  void pumpStream(run, child.stderr, "stderr");

  child.exited.then((code) => {
    run.exitCode = code;
    run.endedAt = new Date().toISOString();
    if (run.status !== "stopped") {
      run.status = code === 0 ? "finished" : "failed";
    }
    appendLog(run, "system", `process exited code=${code}`);
    emit(run, "status", publicRun(run));
  });

  return run;
}

function stopRun(run: RunRecord) {
  if (!run.process || run.status === "finished" || run.status === "failed" || run.status === "stopped") {
    return;
  }
  run.status = "stopped";
  run.endedAt = new Date().toISOString();
  appendLog(run, "system", "stop requested");
  try {
    run.process.kill();
  } catch (error) {
    appendLog(run, "system", `stop error=${String(error)}`);
  }
  emit(run, "status", publicRun(run));
}

async function pumpStream(run: RunRecord, stream: ReadableStream<Uint8Array>, source: LogSource) {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split(/\r?\n/);
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.length > 0) {
        appendLog(run, source, line);
      }
    }
  }

  buffer += decoder.decode();
  if (buffer.trim().length > 0) {
    appendLog(run, source, buffer.trimEnd());
  }
}

function appendLog(run: RunRecord, source: LogSource, text: string) {
  const entry: RunLog = {
    at: new Date().toISOString(),
    source,
    text,
  };
  run.logs.push(entry);
  if (run.logs.length > MAX_LOG_LINES) {
    run.logs.splice(0, run.logs.length - MAX_LOG_LINES);
  }
  emit(run, "log", entry);
}

function emit(run: RunRecord, event: string, payload: unknown) {
  const frame = encodeSse(event, payload);
  for (const controller of run.listeners) {
    try {
      controller.enqueue(frame);
    } catch {
      run.listeners.delete(controller);
    }
  }
}

function sse(run: RunRecord) {
  let currentController: ReadableStreamDefaultController<string> | null = null;
  return new Response(
    new ReadableStream<string>({
      start(controller) {
        currentController = controller;
        run.listeners.add(controller);
        controller.enqueue(encodeSse("status", publicRun(run)));
        for (const log of run.logs) {
          controller.enqueue(encodeSse("log", log));
        }
        const heartbeat = setInterval(() => {
          controller.enqueue(`: heartbeat ${Date.now()}\n\n`);
        }, 15000);
        controller.enqueue(encodeSse("ready", { id: run.id }));
        (controller as ReadableStreamDefaultController<string> & {
          heartbeat?: ReturnType<typeof setInterval>;
        }).heartbeat = heartbeat;
      },
      cancel() {
        if (!currentController) {
          return;
        }
        const controller = currentController as ReadableStreamDefaultController<string> & {
          heartbeat?: ReturnType<typeof setInterval>;
        };
        if (controller.heartbeat) {
          clearInterval(controller.heartbeat);
        }
        run.listeners.delete(currentController);
        currentController = null;
      },
    }),
    {
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "cache-control": "no-cache, no-transform",
        connection: "keep-alive",
      },
    }
  );
}

function encodeSse(event: string, payload: unknown) {
  return `event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`;
}

function publicRun(run: RunRecord) {
  return {
    id: run.id,
    scenarioId: run.scenarioId,
    title: run.title,
    example: run.example,
    status: run.status,
    startedAt: run.startedAt,
    endedAt: run.endedAt,
    pid: run.pid,
    exitCode: run.exitCode,
    env: run.env,
    logCount: run.logs.length,
    lastLog: run.logs.at(-1) ?? null,
  };
}

function buildRunEnv(scenario: Scenario, inputEnv: Record<string, unknown>) {
  const allowed = new Set(scenario.fields.map((field) => field.key));
  const env: Record<string, string> = {};
  for (const field of scenario.fields) {
    env[field.key] = field.defaultValue;
  }
  for (const [key, value] of Object.entries(inputEnv)) {
    if (!allowed.has(key)) {
      continue;
    }
    env[key] = typeof value === "boolean" ? (value ? "1" : "0") : String(value ?? "");
  }
  return env;
}

async function loadCredentials(filePath: string) {
  if (!filePath || !existsSync(filePath)) {
    return {};
  }
  const text = await Bun.file(filePath).text();
  const env: Record<string, string> = {};
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) {
      continue;
    }
    const normalized = line.startsWith("export ") ? line.slice(7).trim() : line;
    const match = normalized.match(/^([A-Za-z_][A-Za-z0-9_]*)=(.*)$/);
    if (!match) {
      continue;
    }
    const [, key, rawValue] = match;
    let value = rawValue.trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    env[key] = value;
  }
  return env;
}

function textField(key: string, label: string, defaultValue: string, description: string): ScenarioField {
  return { key, label, type: "text", defaultValue, description };
}

function numberField(key: string, label: string, defaultValue: string, description: string): ScenarioField {
  return { key, label, type: "number", defaultValue, description };
}

function boolField(key: string, label: string, defaultValue: string, description: string): ScenarioField {
  return { key, label, type: "boolean", defaultValue, description };
}

function selectField(
  key: string,
  label: string,
  defaultValue: string,
  description: string,
  options: Array<{ label: string; value: string }>
): ScenarioField {
  return { key, label, type: "select", defaultValue, description, options };
}

function json(payload: unknown, status = 200) {
  return new Response(JSON.stringify(payload, null, 2), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
    },
  });
}
