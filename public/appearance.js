// Apply the preference before styles and the application load.
(() => {
  const media = matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    let preference;
    try { preference = localStorage.getItem("ferrous-frog-theme"); } catch { /* Use the system theme when storage is unavailable. */ }
    const dark = preference === "dark" || (preference !== "light" && media.matches);
    document.documentElement.classList.toggle("dark", dark);
    document.documentElement.style.colorScheme = dark ? "dark" : "light";
  };
  apply();
  if (document.documentElement.hasAttribute("data-splash")) {
    media.addEventListener("change", apply);
    window.addEventListener("storage", apply);
  }
})();
