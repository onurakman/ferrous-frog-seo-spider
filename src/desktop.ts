if (import.meta.env.PROD) {
  document.addEventListener("contextmenu", (event) => event.preventDefault(), { capture: true });
}
