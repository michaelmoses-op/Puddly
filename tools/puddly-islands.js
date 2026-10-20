/**
 * Puddly island runtime (Step 11 vertical slice).
 *
 * - Finds `[data-puddly-island]` placeholders emitted by SSR
 * - Hydrates by reading `data-puddly-props` (JSON)
 * - Fine-grained updates: patch text/attrs by `data-puddly-id` (no VDOM)
 */
(function (global) {
  "use strict";

  function parseProps(el) {
    var raw = el.getAttribute("data-puddly-props") || "{}";
    try {
      return JSON.parse(raw);
    } catch (_) {
      return {};
    }
  }

  function hydrateIsland(el) {
    var name = el.getAttribute("data-puddly-island") || "";
    var props = parseProps(el);
    el.__puddly = { name: name, props: props };
    return el.__puddly;
  }

  function hydrateAll(root) {
    var scope = root || document;
    var nodes = scope.querySelectorAll("[data-puddly-island]");
    var out = [];
    for (var i = 0; i < nodes.length; i++) {
      out.push(hydrateIsland(nodes[i]));
    }
    return out;
  }

  /**
   * Update a node marked with `data-puddly-id` inside an island (or document).
   * Text nodes / elements: sets textContent. If `attr` is passed, sets that attribute.
   */
  function update(puddlyId, value, opt) {
    opt = opt || {};
    var root = opt.root || document;
    var el = root.querySelector('[data-puddly-id="' + puddlyId + '"]');
    if (!el) return false;
    if (opt.attr) {
      el.setAttribute(opt.attr, String(value));
    } else {
      el.textContent = String(value);
    }
    return true;
  }

  function updateInIsland(islandEl, puddlyId, value, opt) {
    opt = opt || {};
    opt.root = islandEl;
    return update(puddlyId, value, opt);
  }

  var api = {
    hydrateAll: hydrateAll,
    hydrateIsland: hydrateIsland,
    update: update,
    updateInIsland: updateInIsland,
  };

  global.puddlyIslands = api;

  if (typeof document !== "undefined") {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", function () {
        hydrateAll(document);
      });
    } else {
      hydrateAll(document);
    }
  }
})(typeof window !== "undefined" ? window : globalThis);
