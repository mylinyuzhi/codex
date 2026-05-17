function normalizeLanguageName(language) {
  var normalized = (language || "").toLowerCase();
  var aliases = {
    sh: "bash",
    shell: "bash",
    zsh: "bash",
    powershell: "bash",
    ps1: "bash",
    patch: "diff",
    udiff: "diff",
    md: "markdown",
    mdown: "markdown",
    rs: "rust",
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    py: "python",
    yml: "yaml",
    jsonl: "json",
    golang: "go",
    text: "plaintext",
    txt: "plaintext",
  };
  return aliases[normalized] || normalized || "plaintext";
}

function normalizeCodeBlocks(root) {
  (root || document).querySelectorAll("pre code").forEach(function (code) {
    var languageClass = Array.from(code.classList).find(function (className) {
      return className.indexOf("language-") === 0 || className.indexOf("lang-") === 0;
    });
    var language = languageClass
      ? languageClass.replace(/^lang(uage)?-/, "")
      : "plaintext";
    code.className = "language-" + normalizeLanguageName(language);
  });
}

function renderMarkdown(root) {
  if (!window.marked || !window.DOMPurify) {
    return;
  }

  window.marked.setOptions({
    async: false,
    breaks: false,
    gfm: true,
  });

  (root || document)
    .querySelectorAll("[data-render-markdown]")
    .forEach(function (container) {
      var source = container.querySelector("template.render-source");
      if (!source) {
        return;
      }
      var markdown = source.content ? source.content.textContent : source.textContent;
      var rendered = window.marked.parse(markdown);
      container.innerHTML = window.DOMPurify.sanitize(rendered, {
        USE_PROFILES: { html: true },
      });
    });
}

function highlightCode(root) {
  if (window.Prism) {
    normalizeCodeBlocks(root || document);
    window.Prism.highlightAllUnder(root || document);
  }
}

function timelineRows() {
  return Array.from(document.querySelectorAll("#event-stream .event"));
}

function applyLoadedSearch() {
  var input = document.getElementById("timeline-search");
  var status = document.getElementById("timeline-search-status");
  var rows = timelineRows();

  if (!input) {
    return;
  }

  var terms = input.value
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  var visible = 0;

  rows.forEach(function (row) {
    var haystack = [
      row.dataset.msgType,
      row.dataset.lane,
      row.dataset.tool,
      row.dataset.search,
      row.dataset.searchText,
    ]
      .join(" ")
      .toLowerCase();
    var matches = terms.every(function (term) {
      return haystack.indexOf(term) !== -1;
    });
    row.hidden = !matches;
    if (matches) {
      visible += 1;
    }
  });

  if (status) {
    status.textContent = terms.length === 0 ? "" : visible + " / " + rows.length + " rows";
  }
}

function setRawDetails(open) {
  timelineRows().forEach(function (row) {
    row.querySelectorAll("details.event__raw").forEach(function (details) {
      details.open = open;
    });
  });
}

document.addEventListener("htmx:afterSwap", function (event) {
  var root = event.detail && event.detail.target ? event.detail.target : document;
  renderMarkdown(root);
  highlightCode(root);
  applyLoadedSearch();
});

document.addEventListener("DOMContentLoaded", function () {
  renderMarkdown(document);
  highlightCode(document);
  applyLoadedSearch();
});

document.addEventListener("input", function (event) {
  if (event.target && event.target.id === "timeline-search") {
    applyLoadedSearch();
  }
});

document.addEventListener("click", function (event) {
  var button = event.target.closest("[data-detail-action]");
  if (!button) {
    return;
  }
  setRawDetails(button.dataset.detailAction === "expand");
});
