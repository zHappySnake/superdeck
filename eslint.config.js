// SPDX-License-Identifier: GPL-3.0-or-later
import path from "node:path";
import { fileURLToPath } from "node:url";

import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import { defineConfig, globalIgnores } from "eslint/config";
import globals from "globals";
import tseslint from "typescript-eslint";

const pluginsDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "plugins");

/**
 * Bundled plugins must stay independent of the app: a relative import may not
 * leave the plugin's own folder (no reaching into app/, crates/ or other plugins).
 */
const pluginBoundary = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      outside:
        "Plugins must be independent: '{{source}}' resolves outside plugins/{{plugin}}/. Depend on @superdeck/sdk instead.",
    },
  },
  create(context) {
    const rel = path.relative(pluginsDir, context.filename);
    const plugin = rel.split(path.sep)[0];
    if (!plugin || rel.startsWith("..") || path.isAbsolute(rel)) return {};
    const root = path.join(pluginsDir, plugin);

    function check(node, source) {
      if (source?.type !== "Literal" || typeof source.value !== "string") return;
      if (!source.value.startsWith(".")) return;
      const target = path.resolve(path.dirname(context.filename), source.value);
      if (target !== root && !target.startsWith(root + path.sep)) {
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
    files: ["plugins/**"],
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
