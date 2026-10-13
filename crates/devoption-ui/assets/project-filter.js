(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", () => {
    const filter = document.getElementById("project-filter");
    if (!filter) return;
    const query = document.getElementById("project-query");
    const clear = document.getElementById("project-filter-clear");
    const status = document.getElementById("project-filter-status");
    const normalize = (value) => value.normalize("NFC").toLowerCase();
    const items = Array.from(document.querySelectorAll("[data-project-item]")).map((item) => ({
      item, name: normalize(item.querySelector("[data-project-name]").textContent),
    }));
    function update() {
      const value = normalize(query.value.trim());
      let count = 0;
      items.forEach(({ item, name }) => {
        item.hidden = !name.includes(value);
        if (!item.hidden) ++count;
      });
      clear.disabled = query.value.length === 0;
      status.textContent = count === 0 ? "No projects match. Clear the filter to see all projects."
        : value ? `${count} of ${items.length} projects`
          : `${count} ${count === 1 ? "project" : "projects"}`;
    }
    query.addEventListener("input", update);
    clear.addEventListener("click", () => { query.value = ""; update(); query.focus(); });
    // Browsers can restore the field independently of DOM visibility on Back.
    window.addEventListener("pageshow", update);
    update();
    filter.hidden = false;
  });
})();
