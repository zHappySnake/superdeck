// SPDX-License-Identifier: GPL-3.0-or-later
import path from "node:path";
import { fileURLToPath } from "node:url";

import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import { defineConfig, globalIgnores } from "eslint/config";
import globals from "globals";
import tseslint from "typescript-eslint";

const root = path.dirname(fileURLToPath(import.meta.url));
/** Directories of plugins: bundled ones, and the SDK's examples, which authors copy. */
const pluginParents = ["plugins", "sdk/examples"].map((dir) => path.join(root, dir));

/**
 * Plugins must stay independent of the app: a relative import may not leave
 * the plugin's own folder (no reaching into app/, crates/ or other plugins).
 */
const pluginBoundary = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      outside:
        "Plugins must be independent: '{{source}}' resolves outside {{plugin}}/. Depend on @superdeck/sdk instead.",
    },
  },
  create(context) {
    const parent = pluginParents.find((dir) => context.filename.startsWith(dir + path.sep));
    if (!parent) return {};
    const plugin = path.relative(parent, context.filename).split(path.sep)[0];
    const pluginRoot = path.join(parent, plugin);

    function check(node, source) {
      if (source?.type !== "Literal" || typeof source.value !== "string") return;
      if (!source.value.startsWith(".")) return;
      const target = path.resolve(path.dirname(context.filename), source.value);
      if (target !== pluginRoot && !target.startsWith(pluginRoot + path.sep)) {
        context.report({
          node: source,
          messageId: "outside",
          data: { source: source.value, plugin },
        });
      }
    }

    return {
      ImportDeclaration: (node) => check(node, node.source),
      ExportNamedDeclaration: (node) => check(node, node.source),
      ExportAllDeclaration: (node) => check(node, node.source),
      ImportExpression: (node) => check(node, node.source),
      CallExpression: (node) => {
        if (node.callee.type === "Identifier" && node.callee.name === "require") {
          check(node, node.arguments[0]);
        }
      },
    };
  },
};

export default defineConfig(
  globalIgnores([
    "**/target/",
    "**/dist/",
    "**/build/",
    "**/.svelte-kit/",
    "**/*.sdPlugin/bin/",
    "app/src-tauri/gen/",
  ]),
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    languageOptions: {
      globals: { ...globals.node },
    },
  },
  {
    files: ["plugins/**", "sdk/examples/**"],
    plugins: { superdeck: { rules: { "plugin-boundary": pluginBoundary } } },
    rules: {
      "superdeck/plugin-boundary": "error",
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@superdeck/*", "!@superdeck/sdk", "!@superdeck/sdk/*", "superdeck-*"],
              message: "Plugins may only depend on @superdeck/sdk from this repository.",
            },
          ],
        },
      ],
    },
  },
  prettier,
);
