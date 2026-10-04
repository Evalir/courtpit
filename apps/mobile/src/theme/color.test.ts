import { BLACK, WHITE, bestOn, contrast, mix, parseColor, toHex, withContrast } from "./color";

const hex = (value: string) => {
  const parsed = parseColor(value);
  if (!parsed) throw new Error(`unparsable ${value}`);
  return parsed;
};

describe("parseColor", () => {
  it.each([
    ["#0b6e4f", { r: 11, g: 110, b: 79, a: 1 }],
    ["#FFF", { r: 255, g: 255, b: 255, a: 1 }],
    ["#0008", { r: 0, g: 0, b: 0, a: 136 / 255 }],
    ["#00000080", { r: 0, g: 0, b: 0, a: 128 / 255 }],
    ["rgb(10, 20, 30)", { r: 10, g: 20, b: 30, a: 1 }],
    ["rgba(10,20,30,0.5)", { r: 10, g: 20, b: 30, a: 0.5 }],
    ["rgb(100% 0% 0% / 50%)", { r: 255, g: 0, b: 0, a: 0.5 }],
    ["  #abc  ", { r: 170, g: 187, b: 204, a: 1 }],
  ])("parses %s", (input, expected) => {
    expect(parseColor(input)).toEqual(expected);
  });

  it.each(["", "green", "#12", "#12345", "rgb(1,2)", "rgb(a,b,c)", "hsl(0 0% 0%)"])(
    "rejects %p",
    (input) => {
      expect(parseColor(input)).toBeNull();
    },
  );

  it("clamps out-of-range channels", () => {
    expect(parseColor("rgb(300, -5, 10)")).toEqual({ r: 255, g: 0, b: 10, a: 1 });
  });
});

describe("toHex", () => {
  it("round-trips and keeps alpha only when translucent", () => {
    expect(toHex(hex("#0B6E4F"))).toBe("#0b6e4f");
    expect(toHex(hex("#00000080"))).toBe("#00000080");
  });
});

describe("contrast", () => {
  it("matches the WCAG extremes", () => {
    expect(contrast(BLACK, WHITE)).toBeCloseTo(21, 5);
    expect(contrast(WHITE, WHITE)).toBeCloseTo(1, 5);
  });

  it("is symmetric", () => {
    expect(contrast(hex("#0b6e4f"), WHITE)).toBeCloseTo(contrast(WHITE, hex("#0b6e4f")), 10);
  });
});

describe("mix", () => {
  it("interpolates channels", () => {
    expect(toHex(mix(BLACK, WHITE, 0.5))).toBe("#808080");
    expect(mix(BLACK, WHITE, 0)).toEqual(BLACK);
    expect(mix(BLACK, WHITE, 1)).toEqual(WHITE);
  });
});

describe("bestOn", () => {
  it("picks white on dark and black on light", () => {
    expect(bestOn(hex("#0b6e4f"), [WHITE, BLACK])).toBe(WHITE);
    expect(bestOn(hex("#f4b942"), [WHITE, BLACK])).toBe(BLACK);
  });
});

describe("withContrast", () => {
  it("keeps a color that already reads", () => {
    const green = hex("#0b6e4f");
    expect(withContrast(green, WHITE, 4.5)).toBe(green);
  });

  it("darkens a pale brand color on a light background until it reads", () => {
    const yellow = hex("#f4b942");
    const fixed = withContrast(yellow, WHITE, 4.5);
    expect(contrast(fixed, WHITE)).toBeGreaterThanOrEqual(4.5);
    // Still recognisably warm, not black.
    expect(fixed.r).toBeGreaterThan(fixed.b);
  });

  it("lightens towards white on a dark background", () => {
    const navy = hex("#1a237e");
    const fixed = withContrast(navy, hex("#101010"), 4.5);
    expect(contrast(fixed, hex("#101010"))).toBeGreaterThanOrEqual(4.5);
  });
});
