/**
 * Puddly island runtime (Step 11) + declarative RPC mutations (Step 16) +
 * SSE live updates (Step 22).
 *
 * - Finds `[data-puddly-island]` placeholders emitted by SSR
 * - Hydrates by reading `data-puddly-props` (JSON)
 * - Fine-grained updates: patch text/attrs by `data-puddly-id` (no VDOM)
 * - `puddlyIslands.rpc(operation, body)` → POST /rpc/<op> with CSRF + cookies
 * - Binds `data-puddly-rpc` click/submit handlers (forms serialize named inputs)
 * - `puddlyIslands.subscribeLive()` → EventSource('/events'); auto when
 *   `data-puddly-live="1"` on <html> or <body>
 */
(function (global) {
  "use strict";

  /** Must match `puddly_core::CSRF_HEADER_NAME` (HTTP header names are case-insensitive). */
  var CSRF_HEADER = "X-Puddly-CSRF";

  var liveSource = null;

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
    bindRpcActions(scope);
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

  /**
   * Replace island props JSON and patch known fields (title / done) via data-puddly-id.
   */
  function applyIslandProps(islandEl, nextProps) {
    if (!islandEl || !nextProps || typeof nextProps !== "object") return false;
    var merged = Object.assign(
      {},
      islandEl.__puddly && islandEl.__puddly.props ? islandEl.__puddly.props : {},
      nextProps
    );
    islandEl.__puddly = islandEl.__puddly || {};
    islandEl.__puddly.props = merged;
    try {
      islandEl.setAttribute("data-puddly-props", JSON.stringify(merged));
    } catch (_) {}
    if (merged.title != null) updateInIsland(islandEl, "title", merged.title);
    if (merged.done != null) updateInIsland(islandEl, "done", merged.done);
    if (merged.projectTitle != null) {
      updateInIsland(islandEl, "project", merged.projectTitle);
    }
    return true;
  }

  function findIslandByKey(id) {
    if (id == null || typeof document === "undefined") return null;
    var key = String(id);
    return (
      document.querySelector('[data-puddly-key="' + key + '"]') ||
      document.querySelector('[data-puddly-island][data-puddly-key="' + key + '"]')
    );
  }

  /**
   * Apply one SSE JSON event. Prefer patching an island by id/key; otherwise reload.
   */
  function applyLiveEvent(event) {
    if (!event || typeof event !== "object") return;
    var op = event.op || "";
    var data = event.data || {};
    var id = data.id != null ? data.id : data.archived;
    if (op === "update" && id != null) {
      var island = findIslandByKey(id);
      if (island) {
        applyIslandProps(island, data);
        return;
      }
    }
    // create / delete / archive / unknown → full reload so the list stays consistent
    if (typeof location !== "undefined" && location.reload) {
      location.reload();
    }
  }

  /**
   * Open EventSource('/events') and apply live patches. Idempotent.
   */
  function subscribeLive(opt) {
    opt = opt || {};
    if (typeof EventSource === "undefined") return null;
    if (liveSource) return liveSource;
    var url = opt.url || "/events";
    var es = new EventSource(url);
    liveSource = es;
    es.onmessage = function (ev) {
      var payload = null;
      try {
        payload = JSON.parse(ev.data);
      } catch (_) {
        return;
      }
      applyLiveEvent(payload);
    };
    es.onerror = function () {
      // Browser auto-reconnects; leave liveSource set.
    };
    return es;
  }

  function shouldAutoSubscribeLive() {
    if (typeof document === "undefined") return false;
    var root =
      document.documentElement ||
      document.body ||
      (document.querySelector && document.querySelector("html"));
    if (!root || !root.getAttribute) return false;
    if (root.getAttribute("data-puddly-live") === "1") return true;
    if (document.body && document.body.getAttribute("data-puddly-live") === "1") {
      return true;
    }
    return false;
  }

  /**
   * POST /rpc/<operation> with JSON body, CSRF header, and session cookies.
   * Returns parsed `{ ok, data, error }` (network failures → `{ ok: false, error }`).
   */
  function rpc(operation, body) {
    var op = String(operation || "").replace(/^\//, "");
    if (op.indexOf("rpc/") === 0) op = op.slice(4);
    return fetch("/rpc/" + op, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Puddly-CSRF": "1",
      },
      credentials: "include",
      body: JSON.stringify(body == null ? {} : body),
    }).then(
      function (res) {
        return res.json().then(
          function (json) {
            return json;
          },
          function () {
            return {
              ok: false,
              error: {
                code: "http",
                message: "invalid JSON response",
                status: res.status,
              },
            };
          }
        );
      },
      function (err) {
        return {
          ok: false,
          error: {
            code: "network",
            message: String(err && err.message ? err.message : err),
            status: 0,
          },
        };
      }
    );
  }

  /** Parse `data-puddly-body`: JSON object, or bare id string → `{ id }`. */
  function parseBodyAttr(el) {
    var raw = el.getAttribute("data-puddly-body");
    if (raw == null || raw === "") return {};
    try {
      var parsed = JSON.parse(raw);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed;
      }
      if (typeof parsed === "string") {
        return { id: parsed };
      }
      return { id: String(parsed) };
    } catch (_) {
      return { id: raw };
    }
  }

  function closestIsland(el) {
    var cur = el;
    while (cur && cur !== document) {
      if (cur.getAttribute && cur.getAttribute("data-puddly-island") != null) {
        return cur;
      }
      cur = cur.parentNode;
    }
    return null;
  }

  function serializeForm(form) {
    var body = {};
    var elements = form.elements;
    for (var i = 0; i < elements.length; i++) {
      var field = elements[i];
      if (!field.name || field.disabled) continue;
      var tag = (field.tagName || "").toLowerCase();
      if (tag === "button") continue;
      if (field.type === "submit" || field.type === "button" || field.type === "reset") {
        continue;
      }
      if (field.type === "checkbox") {
        body[field.name] = !!field.checked;
        continue;
      }
      if (field.type === "radio") {
        if (field.checked) body[field.name] = field.value;
        continue;
      }
      body[field.name] = field.value;
    }
    return body;
  }

  function shouldReload(el) {
    return el.getAttribute("data-puddly-reload") !== "0";
  }

  /** Prefer `data-puddly-redirect`; default `/` after auth.login / auth.register. */
  function redirectTarget(el) {
    var explicit = el.getAttribute("data-puddly-redirect");
    if (explicit != null && explicit !== "") return explicit;
    var op = el.getAttribute("data-puddly-rpc") || "";
    if (op === "auth.login" || op === "auth.register") return "/";
    return null;
  }

  /** Prefer an existing `[data-puddly-error]` near the control; create one if needed. */
  function errorHost(el) {
    var form = el.closest ? el.closest("form") : null;
    var scope = form || (el.parentNode && el.parentNode.querySelector ? el.parentNode : null);
    var root = document;
    var box =
      (scope && scope.querySelector && scope.querySelector("[data-puddly-error]")) ||
      (root && root.querySelector && root.querySelector("main [data-puddly-error]")) ||
      (root && root.querySelector && root.querySelector("[data-puddly-error]"));
    if (box) return box;
    if (!el.parentNode || typeof document === "undefined") return null;
    box = document.createElement("p");
    box.setAttribute("data-puddly-error", "");
    box.className = "puddly-error";
    if (form) {
      form.insertBefore(box, form.firstChild);
    } else {
      el.parentNode.insertBefore(box, el);
    }
    return box;
  }

  function showRpcError(el, err) {
    var box = errorHost(el);
    var msg =
      err && err.message
        ? String(err.message)
        : err
          ? String(err)
          : "Request failed";
    if (box) box.textContent = msg;
    if (typeof console !== "undefined" && console.error) {
      console.error("puddly rpc failed", err || msg);
    }
  }

  function clearRpcError(el) {
    var box = errorHost(el);
    if (box) box.textContent = "";
  }

  function afterRpc(el, result) {
    if (result && result.ok) {
      clearRpcError(el);
      var dest = redirectTarget(el);
      if (dest != null && typeof location !== "undefined") {
        location.assign(dest);
        return result;
      }
      // When live + reload disabled, patch local island from RPC data (SSE covers peers).
      if (!shouldReload(el)) {
        var island = closestIsland(el);
        if (island && result.data) {
          applyIslandProps(island, result.data);
        }
        return result;
      }
      if (typeof location !== "undefined") {
        location.reload();
      }
    } else {
      showRpcError(el, result && result.error ? result.error : result);
    }
    return result;
  }

  function bodyForControl(el) {
    var body = parseBodyAttr(el);
    var island = closestIsland(el);
    var props = island && island.__puddly ? island.__puddly.props : null;
    if (props && body.id == null && props.id != null) {
      body.id = props.id;
    }
    // Toggle done from island props (restricted TSX can't build JSON templates).
    if (el.getAttribute("data-puddly-toggle-done") != null && props) {
      body.done = !props.done;
    }
    if (el.type === "checkbox" && el.getAttribute("data-puddly-rpc")) {
      body.done = !!el.checked;
    }
    return body;
  }

  function runRpcFrom(el) {
    var operation = el.getAttribute("data-puddly-rpc");
    if (!operation) return Promise.resolve(null);
    var body =
      (el.tagName || "").toLowerCase() === "form"
        ? serializeForm(el)
        : bodyForControl(el);
    return rpc(operation, body).then(function (result) {
      return afterRpc(el, result);
    });
  }

  function bindRpcActions(root) {
    var scope = root || document;
    if (!scope || !scope.querySelectorAll) return;

    var clickables = scope.querySelectorAll(
      "button[data-puddly-rpc], input[data-puddly-rpc], a[data-puddly-rpc], [data-puddly-rpc]:not(form)"
    );
    for (var i = 0; i < clickables.length; i++) {
      var el = clickables[i];
      if (el.__puddlyRpcBound) continue;
      if ((el.tagName || "").toLowerCase() === "form") continue;
      el.__puddlyRpcBound = true;
      if (el.type === "checkbox") {
        el.addEventListener("change", function (ev) {
          ev.preventDefault();
          runRpcFrom(ev.currentTarget);
        });
      } else {
        el.addEventListener("click", function (ev) {
          ev.preventDefault();
          runRpcFrom(ev.currentTarget);
        });
      }
    }

    var forms = scope.querySelectorAll("form[data-puddly-rpc]");
    for (var j = 0; j < forms.length; j++) {
      var form = forms[j];
      if (form.__puddlyRpcBound) continue;
      form.__puddlyRpcBound = true;
      form.addEventListener("submit", function (ev) {
        ev.preventDefault();
        runRpcFrom(ev.currentTarget);
      });
    }
  }

  var api = {
    hydrateAll: hydrateAll,
    hydrateIsland: hydrateIsland,
    update: update,
    updateInIsland: updateInIsland,
    applyIslandProps: applyIslandProps,
    applyLiveEvent: applyLiveEvent,
    subscribeLive: subscribeLive,
    rpc: rpc,
    bindRpcActions: bindRpcActions,
    CSRF_HEADER: CSRF_HEADER,
  };

  global.puddlyIslands = api;

  if (typeof document !== "undefined") {
    function boot() {
      hydrateAll(document);
      if (shouldAutoSubscribeLive()) {
        subscribeLive();
      }
    }
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", boot);
    } else {
      boot();
    }
  }
})(typeof window !== "undefined" ? window : globalThis);
