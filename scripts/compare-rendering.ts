import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { charWidth } from "../src/text";

type Style = { fg: string; bg: string; bold: boolean };
type Cell = Style & { symbol: string };
type Frame = { cells: Cell[][]; cursor: string };
type Case = { name: string; width: number; height: number; padX: number; bordered: boolean };

const cases: Case[] = [
  { name: "default", width: 90, height: 28, padX: 3, bordered: false },
  { name: "narrow", width: 60, height: 20, padX: 1, bordered: false },
  // A 90x28 popup leaves 88x26 content cells when tmux draws a one-cell border.
  { name: "bordered", width: 88, height: 26, padX: 3, bordered: true },
];

const root = resolve(import.meta.dir, "..");
const rustBinary = join(root, "target", "debug", "tmux-ratlette");
const configRoot = mkdtempSync(join(tmpdir(), "tmux-ratlette-render-parity-"));
const socket = `tmux-ratlette-render-${process.pid}`;

function run(command: string[], allowFailure = false): string {
  const result = Bun.spawnSync(command, {
    cwd: root,
    env: { ...process.env, NO_COLOR: "1" },
    stdout: "pipe",
    stderr: "pipe",
  });
  if (!allowFailure && result.exitCode !== 0) {
    const stderr = result.stderr.toString().trim();
    throw new Error(`${command.join(" ")} failed (${result.exitCode}): ${stderr}`);
  }
  return result.stdout.toString();
}

function tmux(...args: string[]): string {
  return run(["tmux", "-L", socket, ...args]);
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

function applySgr(style: Style, parameters: string): void {
  const codes = (parameters || "0").split(";").map((value) => Number(value || 0));
  for (let index = 0; index < codes.length; index++) {
    const code = codes[index]!;
    if (code === 0) {
      style.fg = "default";
      style.bg = "default";
      style.bold = false;
    } else if (code === 1) {
      style.bold = true;
    } else if (code === 22) {
      style.bold = false;
    } else if (code === 39) {
      style.fg = "default";
    } else if (code === 49) {
      style.bg = "default";
    } else if ((code === 38 || code === 48) && codes[index + 1] === 2) {
      const color = `rgb(${codes[index + 2]},${codes[index + 3]},${codes[index + 4]})`;
      if (code === 38) style.fg = color;
      else style.bg = color;
      index += 4;
    } else if (code >= 30 && code <= 37) {
      style.fg = `ansi-${code - 30}`;
    } else if (code >= 40 && code <= 47) {
      style.bg = `ansi-${code - 40}`;
    } else if (code >= 90 && code <= 97) {
      style.fg = `ansi-${code - 82}`;
    } else if (code >= 100 && code <= 107) {
      style.bg = `ansi-${code - 92}`;
    }
  }
}

function parseCapture(capture: string, width: number, height: number): Cell[][] {
  const lines = capture.split("\n");
  const state: Style = { fg: "default", bg: "default", bold: false };
  const cells: Cell[][] = [];

  for (let row = 0; row < height; row++) {
    const line = lines[row] ?? "";
    const output: Cell[] = [];
    for (let index = 0; index < line.length && output.length < width; ) {
      if (line[index] === "\x1b") {
        const match = /^\x1b\[([0-9;]*)m/.exec(line.slice(index));
        if (match) {
          applySgr(state, match[1] ?? "");
          index += match[0].length;
          continue;
        }
      }

      const codePoint = line.codePointAt(index)!;
      const symbol = String.fromCodePoint(codePoint);
      index += symbol.length;
      const glyphWidth = Math.max(1, charWidth(symbol));
      output.push({ symbol, ...state });
      for (
        let continuation = 1;
        continuation < glyphWidth && output.length < width;
        continuation++
      ) {
        output.push({ symbol: "", ...state });
      }
    }
    while (output.length < width) output.push({ symbol: " ", ...state });
    cells.push(output);
  }
  return cells;
}

function comparable(cell: Cell): string {
  // Foreground and bold have no visible effect on a blank terminal cell.
  if (cell.symbol === " " || cell.symbol === "") return `${cell.symbol}|${cell.bg}`;
  return `${cell.symbol}|${cell.fg}|${cell.bg}|${cell.bold}`;
}

function compareFrames(name: string, rust: Frame, typescript: Frame): string[] {
  const differences: string[] = [];
  if (rust.cursor !== typescript.cursor) {
    differences.push(`cursor: Rust=${rust.cursor}, TypeScript=${typescript.cursor}`);
  }
  for (let row = 0; row < rust.cells.length; row++) {
    for (let column = 0; column < rust.cells[row]!.length; column++) {
      const left = rust.cells[row]![column]!;
      const right = typescript.cells[row]![column]!;
      if (comparable(left) !== comparable(right)) {
        differences.push(
          `${name} ${column + 1},${row + 1}: Rust=${comparable(left)} TypeScript=${comparable(right)}`,
        );
      }
    }
  }
  return differences;
}

async function waitForFrame(session: string): Promise<void> {
  for (let attempt = 0; attempt < 50; attempt++) {
    if (tmux("capture-pane", "-p", "-t", `${session}:0.0`).includes("Commands")) return;
    await Bun.sleep(20);
  }
  throw new Error(`${session} did not render within one second`);
}

async function captureImplementation(
  implementation: "rust" | "typescript",
  testCase: Case,
): Promise<Frame> {
  const session = `${testCase.name}-${implementation}`;
  tmux(
    "new-session",
    "-d",
    "-s",
    session,
    "-x",
    String(testCase.width),
    "-y",
    String(testCase.height),
  );
  const executable =
    implementation === "rust"
      ? shellQuote(rustBinary)
      : `bun ${shellQuote(join(root, "src", "cli.ts"))}`;
  const command = [
    `XDG_CONFIG_HOME=${shellQuote(configRoot)}`,
    `TMUX_PALETTE_PADX=${testCase.padX}`,
    `TMUX_PALETTE_BORDERED=${testCase.bordered ? 1 : 0}`,
    executable,
    "commands",
  ].join(" ");
  tmux("send-keys", "-t", `${session}:0.0`, command, "Enter");
  await waitForFrame(session);

  const capture = tmux("capture-pane", "-p", "-e", "-t", `${session}:0.0`);
  const cursor = tmux(
    "display-message",
    "-p",
    "-t",
    `${session}:0.0`,
    "#{cursor_x},#{cursor_y}",
  ).trim();
  return { cells: parseCapture(capture, testCase.width, testCase.height), cursor };
}

let failed = false;
try {
  run(["cargo", "build", "--quiet"]);
  tmux("-f", "/dev/null", "new-session", "-d", "-s", "bootstrap", "-x", "10", "-y", "5");

  for (const testCase of cases) {
    const [rust, typescript] = await Promise.all([
      captureImplementation("rust", testCase),
      captureImplementation("typescript", testCase),
    ]);
    const differences = compareFrames(testCase.name, rust, typescript);
    if (differences.length === 0) {
      console.log(
        `PASS ${testCase.name} ${testCase.width}x${testCase.height} cursor=${rust.cursor}`,
      );
    } else {
      failed = true;
      console.error(`FAIL ${testCase.name}: ${differences.length} differing cells/coordinates`);
      for (const difference of differences.slice(0, 20)) console.error(`  ${difference}`);
    }
  }
} finally {
  run(["tmux", "-L", socket, "kill-server"], true);
  rmSync(configRoot, { recursive: true, force: true });
}

if (failed) process.exit(1);
