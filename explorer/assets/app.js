// LEZ Registry Explorer — client behaviour.
//
// Deliberately tiny and dependency-free. Everything here is presentation only:
// theme preference and copy-to-clipboard. No data is fetched from anywhere but
// the page itself, so the "pure view" claim holds even in the browser.

(function () {
  "use strict";

  // ---- Theme: respect the OS, remember an explicit choice, never call out. ----
  var root = document.documentElement;
  var KEY = "lez-explorer-theme";

  function apply(theme) {
    root.setAttribute("data-theme", theme);
    var btn = document.getElementById("theme-toggle");
    if (btn) {
      btn.textContent = theme === "dark" ? "◐ light" : "◑ dark";
      btn.setAttribute("aria-label", "Switch to " + (theme === "dark" ? "light" : "dark") + " theme");
    }
  }

  var stored = null;
  try { stored = localStorage.getItem(KEY); } catch (e) {}
  if (stored === "light" || stored === "dark") {
    apply(stored);
  } else {
    var prefersLight = window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches;
    apply(prefersLight ? "light" : "dark");
  }

  document.addEventListener("click", function (ev) {
    var t = ev.target;
    if (t && t.id === "theme-toggle") {
      var next = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
      apply(next);
      try { localStorage.setItem(KEY, next); } catch (e) {}
      return;
    }
    if (t && t.classList && t.classList.contains("copy")) {
      var value = t.getAttribute("data-copy");
      if (!value) return;
      var done = function () {
        var was = t.textContent;
        t.textContent = "copied";
        setTimeout(function () { t.textContent = was; }, 1100);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(value).then(done, done);
      } else {
        var ta = document.createElement("textarea");
        ta.value = value;
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand("copy"); } catch (e) {}
        document.body.removeChild(ta);
        done();
      }
    }
  });
})();
