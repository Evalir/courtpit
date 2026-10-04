const expoPreset = require("jest-expo/jest-preset");

/** Unit tests (theme, formatting, grouping) and router-level tests under `jest-expo`. */
module.exports = {
  preset: "jest-expo",
  roots: ["<rootDir>/src"],
  setupFiles: [...expoPreset.setupFiles, "<rootDir>/jest.setup.js"],
  // The preset's list, plus the API client's ESM dependencies.
  transformIgnorePatterns: expoPreset.transformIgnorePatterns.map((pattern) =>
    pattern.startsWith("/node_modules/(?!(")
      ? pattern.replace(
          "/node_modules/(?!(",
          "/node_modules/(?!(openapi-fetch|openapi-react-query|",
        )
      : pattern,
  ),
};
