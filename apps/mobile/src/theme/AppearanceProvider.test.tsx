import { act, fireEvent, render } from "@testing-library/react-native";

import { AppearancePicker } from "@/features/profile/AppearancePicker";
import { Text } from "@/ui/Text";

import { appearanceStore } from "./appearanceStore";
import { AppearanceProvider, useAppearance } from "./AppearanceProvider";

let mockSystemScheme: "light" | "dark" | null = "dark";
jest.mock("react-native/Libraries/Utilities/useColorScheme", () => ({
  __esModule: true,
  default: () => mockSystemScheme,
}));

jest.mock("./appearanceStore", () => ({
  appearanceStore: {
    initial: jest.fn(() => null),
    load: jest.fn(async () => "system"),
    save: jest.fn(async () => {}),
    apply: jest.fn(),
  },
}));
const store = jest.mocked(appearanceStore);

function Scheme() {
  return <Text>{`drawing ${useAppearance().scheme}`}</Text>;
}

const renderPicker = () =>
  render(
    <AppearanceProvider>
      <Scheme />
      <AppearancePicker />
    </AppearanceProvider>,
  );

beforeEach(() => {
  mockSystemScheme = "dark";
  jest.clearAllMocks();
});

describe("AppearanceProvider", () => {
  it("follows the device until the player picks a scheme, and remembers the pick", async () => {
    const view = await renderPicker();
    expect(await view.findByText("drawing dark")).toBeVisible();
    expect(view.getByRole("tab", { name: "System" })).toBeSelected();
    expect(store.apply).toHaveBeenCalledWith("system");

    await act(() => fireEvent.press(view.getByRole("tab", { name: "Light" })));

    expect(view.getByText("drawing light")).toBeVisible();
    expect(view.getByRole("tab", { name: "Light" })).toBeSelected();
    expect(view.getByText("On this device only.")).toBeVisible();
    expect(store.apply).toHaveBeenLastCalledWith("light");
    expect(store.save).toHaveBeenCalledWith("light");
  });

  it("starts from the stored choice", async () => {
    store.load.mockResolvedValueOnce("light");
    const view = await renderPicker();
    expect(await view.findByText("drawing light")).toBeVisible();
    expect(view.getByRole("tab", { name: "Light" })).toBeSelected();
  });

  it("draws light when the device reports no scheme", async () => {
    mockSystemScheme = null;
    const view = await renderPicker();
    expect(await view.findByText("drawing light")).toBeVisible();
  });
});
