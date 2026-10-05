/**
 * Browser session storage: nothing to store. Sign-in sends `X-RacquetCollective-Client: web`, the
 * server answers with an httpOnly cookie, and the browser attaches it to same-origin requests;
 * script never sees the session (decision 10).
 */
export const sessionToken = {
  stored: false,
  current: (): string | undefined => undefined,
  load: async (): Promise<string | undefined> => undefined,
  save: async (_token: string): Promise<void> => {},
  clear: async (): Promise<void> => {},
};
