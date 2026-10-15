// window.pictogrep for plugins running inside Ateliet: the same calls as
// Pictogrep's plugin API (version 0), relayed to the host page with
// postMessage. The host checks every call against the plugin's manifest.
window.pictogrep = (function () {
  let next = 1;
  const pending = new Map();
  window.addEventListener("message", (event) => {
    if (event.source !== window.parent) return;
    const { callId, ok, result, error } = event.data || {};
    const waiting = callId && pending.get(callId);
    if (!waiting) return;
    pending.delete(callId);
    ok ? waiting.resolve(result) : waiting.reject(new Error(error));
  });
  function call(method, args) {
    return new Promise((resolve, reject) => {
      const callId = "pg" + next++;
      pending.set(callId, { resolve, reject });
      window.parent.postMessage({ callId, method, args }, "*");
    });
  }
  return {
    images: {
      list: (options = {}) => call("images.list", options),
      search: (query) => call("images.search", { query }),
      read: async (id) => (await call("images.read", { id })).image,
      tag: (id, tags) => Promise.all((Array.isArray(tags) ? tags : [tags]).map((tag) => call("images.tag", { id, tag }))),
      reveal: (id) => call("images.reveal", { imageId: id }),
    },
    storage: {
      get: async (key) => (await call("storage.kv.get", {})).value?.[key],
      set: (key, value) => call("storage.kv.set", { key, value }),
    },
    ui: {
      openExternal: (url) => call("ui.openExternal", { url }),
    },
  };
})();
