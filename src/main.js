// In Cahoots Studio — v0 spike
//
// Renders the Strategic Thinking session receipt as the first
// item in the feed. AI workflow runner wires up in the next build.

const STRATEGIC_THINKING = {
  id: "rcpt_2026-05-02_07-45-12",
  project: "in-cahoots-studio",
  workflow: "strategic-thinking",
  title: "RECEIPT — STRATEGIC THINKING SESSION",
  date: "Saturday 02 May 2026",
  sections: [
    {
      items: [
        { qty: "1", text: "Original idea: freelancer pool / collective arm" },
        { qty: "✓", text: "Pushback received and integrated" },
        { qty: "✓", text: "Reference research (Jane Abrahami)" },
        { qty: "✓", text: "Model interrogation" },
        { qty: "1", text: "Structural pivot: bid-together → studio-led with subcontractors" },
        { qty: "✓", text: "Domain repurposed: incahoots.cool" },
        { qty: "1", text: "Acronym landed: COOL" },
        { qty: "1", text: "Acronym expansion: Collective Of Organised Logic" },
        { qty: "✓", text: "Strategic coherence with Context noticed and named" },
        { qty: "1", text: "Member nomenclature: cool people" },
        { qty: "✓", text: "Legal vehicle confirmed: In Cahoots Group Pty Ltd" },
        { qty: "1", text: "Two-tier shape: studio + community" },
        { qty: "✓", text: "Sequencing: post-wedding launch" },
      ],
    },
    {
      header: "BONUS ITEMS (no extra charge)",
      items: [
        { qty: "1", text: "Context rebrand → cntxt" },
        { qty: "1", text: "Domain plan: cntxt.studio" },
        { qty: "✓", text: "Trademark logic clarified" },
        { qty: "1", text: "Product family architecture: studio + collective + tools" },
      ],
    },
  ],
  position: {
    header: "POSITION ESTABLISHED",
    quote:
      "In Cahoots provides organised logic for the indie cultural sector — through the studio, through COOL, and through cntxt.",
  },
  totals: [
    { label: "SUBTOTAL", value: "$0.00" },
    { label: "GST", value: "$0.00" },
    { label: "TOTAL", value: "$0.00", grand: true },
  ],
  paid_block: {
    stamp: "PAID IN FULL",
    method: "Saturday morning thinking",
    issued_by: "a thinking partner",
    customer: "Caitlin Reilly",
    status: "fully fkn sick",
  },
  footer_note: "Thank you for your business.",
};

// Tiny helper for DOM construction.
function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") node.className = v;
    else if (k === "style") node.setAttribute("style", v);
    else if (k === "data") {
      for (const [dk, dv] of Object.entries(v)) node.dataset[dk] = dv;
    } else if (k in node) node[k] = v;
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null) continue;
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

function renderItem(item) {
  if (item.type === "task") {
    const checkbox = el("input", { type: "checkbox", checked: item.done ?? false });
    const label = el("span", { class: "task-label" }, [item.text]);
    const qty = el("span", { class: "receipt-item-qty" }, [item.qty || "☐"]);
    const row = el(
      "div",
      {
        class: "receipt-item receipt-item-task" + (item.done ? " done" : ""),
      },
      [el("label", {}, [checkbox, label]), qty]
    );
    return row;
  }
  return el("div", { class: "receipt-item" }, [
    el("div", { class: "receipt-item-text" }, [item.text]),
    el("div", { class: "receipt-item-qty" }, [item.qty || ""]),
  ]);
}

function renderReceipt(receipt) {
  const root = el("article", { class: "receipt" });
  root.dataset.receiptId = receipt.id;

  // Header
  root.appendChild(
    el("div", { class: "receipt-header" }, [
      el("div", { class: "receipt-title" }, [receipt.title]),
      el("div", { class: "receipt-meta" }, [receipt.date]),
    ])
  );

  // Sections
  receipt.sections.forEach((section, idx) => {
    if (section.header) {
      root.appendChild(
        el("div", { class: "receipt-section-label" }, [section.header])
      );
    }
    section.items.forEach((item) => root.appendChild(renderItem(item)));
    if (idx < receipt.sections.length - 1) {
      root.appendChild(el("div", { class: "receipt-divider" }));
    }
  });

  // Position quote
  if (receipt.position) {
    root.appendChild(el("div", { class: "receipt-divider" }));
    root.appendChild(
      el("div", { class: "receipt-section-label" }, [receipt.position.header])
    );
    root.appendChild(
      el("div", { class: "receipt-position" }, [`"${receipt.position.quote}"`])
    );
  }

  // Totals
  if (receipt.totals && receipt.totals.length) {
    root.appendChild(el("div", { class: "receipt-divider" }));
    const totals = el("div", { class: "receipt-totals" });
    receipt.totals.forEach((t) => {
      totals.appendChild(el("div", { class: t.grand ? "total" : "" }, [t.label]));
      totals.appendChild(el("div", { class: t.grand ? "total" : "" }, [t.value]));
    });
    root.appendChild(totals);
  }

  // Paid stamp + meta
  if (receipt.paid_block) {
    root.appendChild(el("div", { class: "receipt-stamp" }, [receipt.paid_block.stamp]));
    const meta = el("div", { class: "receipt-paid-meta" });
    [
      `Payment method: ${receipt.paid_block.method}`,
      `Issued by: ${receipt.paid_block.issued_by}`,
      `Customer: ${receipt.paid_block.customer}`,
      `Status: ${receipt.paid_block.status}`,
    ].forEach((line) => meta.appendChild(el("div", {}, [line])));
    root.appendChild(meta);
  }

  // Footer note
  if (receipt.footer_note) {
    root.appendChild(el("div", { class: "receipt-footer" }, [receipt.footer_note]));
  }

  return root;
}

document.addEventListener("DOMContentLoaded", () => {
  const feed = document.getElementById("feed");
  if (feed) feed.appendChild(renderReceipt(STRATEGIC_THINKING));

  // Tick handler for any future task receipts.
  feed?.addEventListener("change", (e) => {
    if (e.target.matches('input[type="checkbox"]')) {
      const row = e.target.closest(".receipt-item-task");
      if (row) row.classList.toggle("done", e.target.checked);
    }
  });

  document.getElementById("strategic-thinking-btn")?.addEventListener("click", () => {
    alert("Workflow runner wires up next build. The spike proves the architecture.");
  });
});
