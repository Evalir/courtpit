// https://docs.expo.dev/guides/using-eslint/
const { defineConfig } = require("eslint/config");
const expoConfig = require("eslint-config-expo/flat");

module.exports = defineConfig([
  expoConfig,
  {
    ignores: ["dist/*", ".expo/*", "expo-env.d.ts"],
  },
  {
    rules: {
      "no-console": ["warn", { allow: ["warn", "error"] }],
      // `x == null` is the idiomatic null-or-undefined check (API fields are often both).
      eqeqeq: ["error", "always", { null: "ignore" }],
    },
  },
]);
