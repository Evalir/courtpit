// What a native build bakes in (src/api/config.ts); tests stub `fetch`, so the URL is never hit.
process.env.EXPO_PUBLIC_COMMUNITY = "demo";
process.env.EXPO_PUBLIC_API_URL = "https://api.test";

// Unmounted queries would otherwise schedule five-minute garbage-collection timers that keep
// Jest from exiting (TanStack Query's advice for tests: gcTime Infinity).
jest.mock("./src/api/queryClient", () => {
  const actual = jest.requireActual("./src/api/queryClient");
  return {
    ...actual,
    createQueryClient: () => {
      const client = actual.createQueryClient();
      const defaults = client.getDefaultOptions();
      client.setDefaultOptions({ ...defaults, queries: { ...defaults.queries, gcTime: Infinity } });
      return client;
    },
  };
});
