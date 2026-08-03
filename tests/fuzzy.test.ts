import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { defaultFilter, multiFuzzyScore } from "../src/fuzzy";
import type { Item } from "../src/types";

const noop = { shell: ":" } as const;

describe("multiFuzzyScore", () => {
  test("requires every query part to match", () => {
    expect(multiFuzzyScore("split horizontal pane", ["split", "pane"])).toBeGreaterThan(0);
    expect(multiFuzzyScore("split horizontal pane", ["split", "window"])).toBe(0);
  });
});

describe("defaultFilter", () => {
  const items: Item[] = [
    { title: "Split Horizontal", category: "Panes", action: noop },
    { title: "New Window", category: "Windows", action: noop },
    { title: "Choose Session", aliases: ["sessions"], action: noop },
  ];

  test("matches title initials through auto aliases", () => {
    expect(defaultFilter(items, "sh").map((i) => i.title)).toEqual(["Split Horizontal"]);
  });

  test("matches explicit aliases", () => {
    expect(defaultFilter(items, "sessions").map((i) => i.title)).toEqual(["Choose Session"]);
  });

  test("auto-alias outranks substring matches inside a category", () => {
    // "ns" is the auto-alias for both "New Session" and "Next Session".
    // It should rank them above "Detach", which only matches via the
    // "ns" sitting inside the word "Sessions" in its category.
    const sessionItems: Item[] = [
      { title: "Detach", category: "Sessions", action: noop },
      { title: "New Session", category: "Sessions", action: noop },
      { title: "Next Session", category: "Sessions", action: noop },
    ];
    const ranked = defaultFilter(sessionItems, "ns").map((i) => i.title);
    expect(ranked[0]).toBe("New Session");
    expect(ranked[1]).toBe("Next Session");
  });
});

describe("shared Rust/TypeScript parity fixtures", () => {
  const fixture = readFileSync(new URL("./fixtures/fuzzy-parity.tsv", import.meta.url), "utf8");

  for (const line of fixture.split("\n")) {
    if (!line.trim() || line.startsWith("#")) continue;
    const [query, itemSpecs = "", expected = ""] = line.split("\t");
    const fixtureItems: Item[] = itemSpecs.split("|").map((spec) => {
      const [title = "", category = "", aliases = ""] = spec.split("~");
      return {
        title,
        category: category || undefined,
        aliases: aliases ? aliases.split(",") : undefined,
        action: noop,
      };
    });

    test(`ranks ${JSON.stringify(query)}`, () => {
      const actual = defaultFilter(fixtureItems, query).map((item) => item.title);
      expect(actual).toEqual(expected ? expected.split("|") : []);
    });
  }
});
