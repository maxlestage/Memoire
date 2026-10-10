// Applies the saved theme and language before the WebAssembly app starts,
// so returning visitors never see a flash of the wrong theme.
(function () {
  try {
    var root = document.documentElement;
    var theme = localStorage.getItem("memoire-theme");
    if (theme !== "light" && theme !== "dark") {
      theme = window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
    }
    root.setAttribute("data-theme", theme);
    var lang = (localStorage.getItem("memoire-lang") || navigator.language || "en").slice(0, 2).toLowerCase();
    if (lang === "fr" || lang === "es" || lang === "en") root.setAttribute("lang", lang);
  } catch (e) {}
})();
