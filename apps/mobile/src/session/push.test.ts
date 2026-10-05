import { renderHook } from "@testing-library/react-native";
import * as Notifications from "expo-notifications";
import { router } from "expo-router";

import { useNotificationTaps } from "./push";
import { returnTo } from "./returnTo";

jest.mock("expo-router", () => ({ router: { push: jest.fn() } }));

const tapped = (url: string) =>
  ({ notification: { request: { content: { data: { url } } } } }) as never;

describe("useNotificationTaps", () => {
  it("opens the launch notification once, then each tap; after sign-in when signed out", async () => {
    jest.mocked(Notifications.getLastNotificationResponse).mockReturnValue(tapped("/matches/m1"));
    let onTap: (response: never) => void = () => undefined;
    jest
      .mocked(Notifications.addNotificationResponseReceivedListener)
      .mockImplementation((listener) => {
        onTap = listener as never;
        return { remove: jest.fn() } as never;
      });

    const view = await renderHook(
      ({ signedIn }: { signedIn: boolean }) => useNotificationTaps(signedIn),
      {
        initialProps: { signedIn: false },
      },
    );
    expect(router.push).not.toHaveBeenCalled();
    expect(returnTo.take()).toBe("/matches/m1");

    await view.rerender({ signedIn: true });
    expect(router.push).not.toHaveBeenCalled(); // the launch notification was already handled
    onTap(tapped("/leagues/l1"));
    expect(router.push).toHaveBeenCalledWith("/leagues/l1");
    onTap(tapped("/sign-in"));
    expect(router.push).toHaveBeenCalledTimes(1);
  });
});
