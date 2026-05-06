// Skills overflow modal (v0.34).
//
// Triggered from Today's "More skills..." button. Lists every skill in
// the registry, grouped by category. Click a card to dispatch with the
// current view's context (Today has no client/project, so contextual
// pre-fill is empty — the underlying modal opens its own picker).

import { skillsByCategory, CATEGORIES } from "./registry.js";

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null) continue;
    if (k === "class") node.className = v;
    else if (k === "style") node.setAttribute("style", v);
    else if (k in node) node[k] = v;
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null || c === false) continue;
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

export function show(onPick) {
  if (document.getElementById("skills-overflow-modal")) return;

  const overlay = el("div", {
    id: "skills-overflow-modal",
    class: "modal-overlay",
  });
  const modal = el("div", { class: "modal modal-wide" });
  const close = () => overlay.remove();

  modal.appendChild(
    el("div", { class: "modal-header" }, [
      el("div", { class: "modal-title" }, ["All skills"]),
      el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
    ])
  );
  modal.appendChild(
    el("div", { class: "modal-meta" }, [
      "Pick a skill. Pre-fills client or project context where the modal supports it.",
    ])
  );

  const body = el("div", { class: "modal-pad skills-overflow-body" });
  const grouped = skillsByCategory();
  CATEGORIES.forEach((cat) => {
    const skills = grouped.get(cat.key) || [];
    if (skills.length === 0) return;
    body.appendChild(
      el("div", { class: "section-label skills-overflow-group-label" }, [cat.label])
    );
    const grid = el("div", { class: "client-shortcut-grid skills-overflow-grid" });
    skills.forEach((s) => {
      const card = el(
        "button",
        {
          class:
            "client-shortcut" +
            (s.placeholder ? " client-shortcut-placeholder" : ""),
          type: "button",
          "data-skill-id": s.id,
        },
        [
          el("div", { class: "client-shortcut-title" }, [s.label]),
          el("div", { class: "client-shortcut-meta" }, [
            s.description || "",
          ]),
        ]
      );
      card.addEventListener("click", () => {
        close();
        onPick?.(s.id);
      });
      grid.appendChild(card);
    });
    body.appendChild(grid);
  });
  modal.appendChild(body);

  const actions = el("div", { class: "modal-actions" });
  const cancel = el(
    "button",
    { class: "button button-secondary", type: "button" },
    ["Close"]
  );
  cancel.addEventListener("click", close);
  actions.appendChild(cancel);
  modal.appendChild(actions);

  overlay.appendChild(modal);
  document.body.appendChild(overlay);

  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  modal.querySelector(".modal-close").addEventListener("click", close);
}
