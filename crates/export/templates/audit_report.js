"use strict";
const search = document.getElementById("finding-search");
const severity = document.getElementById("severity-filter");
const team = document.getElementById("team-filter");
function filterFindings() {
  let visible = 0;
  for (const card of document.querySelectorAll("[data-finding]")) {
    card.hidden = Boolean((severity.value && card.dataset.severity !== severity.value) ||
      (team.value && card.dataset.team !== team.value) ||
      !card.textContent.toLocaleLowerCase().includes(search.value.toLocaleLowerCase()));
    if (!card.hidden) visible += 1;
  }
  document.getElementById("finding-count").textContent = String(visible);
}
if (search && severity && team) {
  search.addEventListener("input", filterFindings);
  severity.addEventListener("change", filterFindings);
  team.addEventListener("change", filterFindings);
}
const pageSearch = document.getElementById("page-search");
if (pageSearch) pageSearch.addEventListener("input", () => {
  const query = pageSearch.value.toLocaleLowerCase();
  for (const row of document.querySelectorAll(".evidence-row")) {
    row.hidden = !row.textContent.toLocaleLowerCase().includes(query);
  }
});
