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

// Push needs a device; tests run without one and with nothing tapped.
jest.mock("expo-notifications", () => ({
  setNotificationHandler: jest.fn(),
  getPermissionsAsync: jest.fn(async () => ({ granted: false, canAskAgain: true })),
  requestPermissionsAsync: jest.fn(async () => ({ granted: false })),
  getExpoPushTokenAsync: jest.fn(),
  setNotificationChannelAsync: jest.fn(),
  getLastNotificationResponse: jest.fn(() => null),
  addNotificationResponseReceivedListener: jest.fn(() => ({ remove: jest.fn() })),
  AndroidImportance: { DEFAULT: 3 },
}));
jest.mock("expo-device", () => ({ isDevice: false }));
