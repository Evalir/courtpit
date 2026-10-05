import { render } from "@testing-library/react-native";
import { ExpoRoot } from "expo-router";
import * as SecureStore from "expo-secure-store";
// Also installs the router's Jest mocks (linking, native modules).
import { getMockContext } from "expo-router/testing-library";

import { themeFromBranding } from "@/theme/theme";

// The device's light/dark setting; light unless a test says otherwise.
let mockSystemScheme: "light" | "dark" = "light";
jest.mock("react-native/Libraries/Utilities/useColorScheme", () => ({
  __esModule: true,
  default: () => mockSystemScheme,
}));
afterEach(() => {
  mockSystemScheme = "light";
});

// The demo community as `GET /api/v1/tenant` returns it.
const tenant = {
  slug: "demo",
  name: "Riverside Tennis Club",
  branding: {
    display_name: "Riverside Tennis Club",
    logo_url: null,
    colors: { primary: "#0b6e4f", secondary: "#f4b942" },
    typography: null,
    feature_flags: {},
  },
};

type Handler = (request: Request) => { status: number; body?: unknown };

/** Stubs `fetch` with per-path answers and records what the app asked for. */
function stubApi(routes: Record<string, Handler>) {
  const requests: Request[] = [];
  globalThis.fetch = jest.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init);
    requests.push(request);
    const pathname = new URL(request.url).pathname;
    // Exact paths first, then prefixes ending in "/" (e.g. "/api/v1/players/").
    const handler =
      routes[pathname] ??
      Object.entries(routes).find(([key]) => key.endsWith("/") && pathname.startsWith(key))?.[1];
    const { status, body } = handler
      ? handler(request)
      : { status: 404, body: { error: { code: "not_found", message: "no stub" } } };
    return new Response(body === undefined ? null : JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;
  return requests;
}

// Jest runs this file as CommonJS; the app's tsconfig has no Node types.
declare const __dirname: string;
const appDir = `${__dirname}/app`;
// Boot is several round trips (tenant, session, screen data); the default 1 s is tight on CI.
const slow = { timeout: 5000 };

// `renderRouter` from expo-router 57 predates RNTL 14's async `render`, so mount the router root
// with the same mock route context directly.
const renderApp = (location = "/") =>
  render(<ExpoRoot context={getMockContext(appDir)} location={location} />);

describe("boot", () => {
  it("themes the app from the tenant and sends a signed-out visitor to sign-in", async () => {
    const requests = stubApi({ "/api/v1/tenant": () => ({ status: 200, body: tenant }) });

    // A deep link to a signed-in screen.
    const view = await renderApp("/rankings");

    expect(await view.findByText("Sign in or join", {}, slow)).toBeVisible();
    expect(view.getByText("Riverside Tennis Club")).toBeVisible();
    // Native: the community header on every request; no stored token, so no session check.
    expect(requests.map((request) => new URL(request.url).pathname)).toEqual(["/api/v1/tenant"]);
    expect(requests[0]?.headers.get("X-RacquetCollective-Community")).toBe("demo");
  });

  it("draws the community's dark theme on a device set to dark", async () => {
    mockSystemScheme = "dark";
    stubApi({ "/api/v1/tenant": () => ({ status: 200, body: tenant }) });

    const view = await renderApp();

    const heading = await view.findByText("Sign in or join", {}, slow);
    const { colors } = themeFromBranding(tenant.branding, "dark");
    expect(heading).toHaveStyle({ color: colors.text });
    expect(colors.text).not.toBe(themeFromBranding(tenant.branding, "light").colors.text);
  });

  it("explains a failed boot and offers a retry", async () => {
    stubApi({
      "/api/v1/tenant": () => ({
        status: 404,
        body: { error: { code: "not_found", message: "community not found" } },
      }),
    });

    const view = await renderApp();

    expect(await view.findByText("community not found", {}, slow)).toBeVisible();
    expect(view.getByRole("button", { name: "Try again" })).toBeVisible();
  });

  it("restores a stored session and shows what needs the player", async () => {
    const me = "00000000-0000-7000-8000-000000000001";
    const opponent = "00000000-0000-7000-8000-000000000002";
    jest.spyOn(SecureStore, "getItemAsync").mockResolvedValue("stored-token");
    const requests = stubApi({
      "/api/v1/tenant": () => ({ status: 200, body: tenant }),
      "/api/v1/auth/session": () => ({
        status: 200,
        body: {
          user_id: "u1",
          email: "lily@example.com",
          email_verified: true,
          player_id: me,
          role: "player",
        },
      }),
      "/api/v1/me": () => ({
        status: 200,
        body: { account: {}, player: { display_name: "Lily Fernandez" } },
      }),
      "/api/v1/me/entries": () => ({
        status: 200,
        body: [
          {
            league: {
              id: "l1",
              name: "Autumn Doubles",
              discipline: "doubles",
              status: "registration",
              registration_opens_at: "2020-01-01T00:00:00Z",
              registration_closes_at: "2999-01-01T00:00:00Z",
              starts_at: "2999-01-02T00:00:00Z",
              ends_at: "2999-03-01T00:00:00Z",
              match_format: {
                sets_to_win: 2,
                games_per_set: 6,
                tiebreak_at: 6,
                final_set: "full_set",
              },
              box_min_size: 6,
              box_max_size: 8,
              created_at: "2026-09-01T00:00:00Z",
            },
            entry: {
              id: "e1",
              league_id: "l1",
              player_ids: [opponent],
              created_by: opponent,
              invited_partner_id: me,
              status: "pending_partner",
              looking_for_partner: false,
              created_at: "2026-09-02T00:00:00Z",
              names: [{ id: opponent, display_name: "Mateo Alvarez" }],
            },
          },
        ],
      }),
      "/api/v1/matches": () => ({
        status: 200,
        body: {
          items: [
            {
              id: "m1",
              discipline: "singles",
              side_a: [opponent],
              side_b: [me],
              status: "reported",
              reported_by: opponent,
              score: {
                sets: [
                  { a: 6, b: 2 },
                  { a: 6, b: 3 },
                ],
              },
              winner_side: "a",
              match_format: {
                sets_to_win: 2,
                games_per_set: 6,
                tiebreak_at: 6,
                final_set: "full_set",
              },
              created_at: "2026-09-01T00:00:00Z",
              names: [{ id: opponent, display_name: "Mateo Alvarez" }],
            },
          ],
          next_cursor: null,
        },
      }),
    });

    const view = await renderApp("/");

    expect(await view.findByText("Hi, Lily", {}, slow)).toBeVisible();
    expect(await view.findByText("1 match and 1 league entry need you.", {}, slow)).toBeVisible();
    expect(
      view.getByText("Mateo Alvarez invited you to play Autumn Doubles together"),
    ).toBeVisible();
    expect(await view.findByText("You vs Mateo Alvarez", {}, slow)).toBeVisible();
    // The viewer's games first.
    expect(view.getByText("2–6 3–6")).toBeVisible();
    // Names come with the matches: no request per player.
    expect(requests.some((request) => request.url.includes("/api/v1/players/"))).toBe(false);
    const session = requests.find((request) => request.url.endsWith("/api/v1/auth/session"));
    expect(session?.headers.get("Authorization")).toBe("Bearer stored-token");
  });
});
