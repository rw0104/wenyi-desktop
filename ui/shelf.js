// Shelf rendering: the book list, its covers, and the running progress readout.
//
// Split out of main.js because the shelf has enough state of its own (cover cache, layout
// mode, progress formatting) that mixing it into the run control made both harder to follow.

const SHELF = (() => {
  /** Covers are data URLs; a session cache avoids re-fetching them on every repaint. */
  const coverCache = new Map();
  let books = [];
  let selected = null;

  /** Deterministic hue from a title, so a book keeps its colour between sessions. */
  function tintFor(title) {
    let hash = 0;
    for (let i = 0; i < title.length; i += 1) {
      hash = (hash * 31 + title.charCodeAt(i)) % 360;
    }
    return `hsl(${hash} 32% 34%)`;
  }

  function baseName(path) {
    return path.split(/[\\/]/).pop() || path;
  }

  function displayTitle(book) {
    return book.title || baseName(book.input).replace(/\.[^.]+$/, "");
  }

  /** Fetch a cover once per input and remember the result, including "no cover". */
  async function coverFor(input) {
    if (coverCache.has(input)) return coverCache.get(input);
    let url = null;
    try {
      url = await call("book_cover", { input });
    } catch (error) {
      url = null;
    }
    coverCache.set(input, url);
    return url;
  }

  function buildCover(book, title) {
    const cover = document.createElement("div");
    cover.className = "book-cover";
    cover.style.setProperty("--book-tint", tintFor(title));

    // Formats without cover art get a typographic card rather than an empty hole.
    const placeholder = document.createElement("div");
    placeholder.className = "book-placeholder";
    const glyph = document.createElement("span");
    glyph.className = "glyph";
    glyph.textContent = title.trim().slice(0, 2) || "书";
    const banner = document.createElement("span");
    banner.className = "banner";
    banner.textContent = (book.input.split(".").pop() || "").toUpperCase().slice(0, 5);
    placeholder.append(glyph, banner);
    cover.append(placeholder);

    coverFor(book.input).then((url) => {
      if (!url) return;
      const img = document.createElement("img");
      img.alt = "";
      img.src = url;
      img.addEventListener("load", () => placeholder.remove(), { once: true });
      cover.append(img);
    });
    return cover;
  }

  function buildBook(book, index) {
    const title = displayTitle(book);
    const item = document.createElement("li");
    item.className = "book";
    item.dataset.input = book.input;
    item.style.setProperty("--i", String(index));
    item.tabIndex = 0;
    item.setAttribute("role", "button");
    item.setAttribute("aria-pressed", String(book.input === selected));
    if (book.input === selected) item.classList.add("selected");
    item.title = book.input;

    item.append(buildCover(book, title));

    const meta = document.createElement("div");
    meta.className = "book-meta";

    const heading = document.createElement("div");
    heading.className = "book-title";
    heading.textContent = title;
    meta.append(heading);

    if (!book.inputExists) {
      const missing = document.createElement("div");
      missing.className = "book-sub book-missing";
      missing.textContent = "文件已移动或删除";
      meta.append(missing);
    } else if (book.chaptersTotal > 0) {
      const bar = document.createElement("div");
      bar.className = "progress-bar";
      const fill = document.createElement("div");
      fill.className = "fill";
      fill.style.setProperty("--p", String(book.chaptersDone / book.chaptersTotal));
      bar.append(fill);
      const sub = document.createElement("div");
      sub.className = "book-sub muted";
      // "11/21 章" alone sat directly above the run line's "第 12/21 章" and read as a
      // contradiction: the two numbers measure different things (finished vs in progress),
      // but the shared notation made them look like the same counter disagreeing with itself.
      sub.textContent = `已完成 ${book.chaptersDone}/${book.chaptersTotal} 章`;
      meta.append(bar, sub);
    } else {
      const sub = document.createElement("div");
      sub.className = "book-sub muted";
      sub.textContent = "尚未开始";
      meta.append(sub);
    }
    item.append(meta);

    // Removing only takes the book off the shelf; the file is untouched, so this is
    // reversible and needs no confirmation dialog.
    const remove = document.createElement("button");
    remove.className = "book-remove";
    remove.type = "button";
    remove.textContent = "×";
    remove.title = "从书架移除（不删除文件）";
    remove.setAttribute("aria-label", `从书架移除 ${title}`);
    remove.addEventListener("click", async (event) => {
      event.stopPropagation();
      books = await call("remove_book", { input: book.input });
      coverCache.delete(book.input);
      if (selected === book.input) selected = null;
      render();
    });
    item.append(remove);

    const choose = () => select(book.input);
    item.addEventListener("click", choose);
    item.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        choose();
      }
    });
    return item;
  }

  function render() {
    const list = $("shelf-list");
    const empty = $("shelf-empty");
    const shelf = $("shelf");
    const addButton = $("add-books");

    const hasBooks = books.length > 0;
    empty.hidden = hasBooks;
    shelf.hidden = !hasBooks;
    addButton.hidden = !hasBooks;
    if (window.__runningGuard) window.__runningGuard();

    list.textContent = "";
    // A single book is shown at full size; only from two onward is it a shelf of spines.
    list.classList.toggle("single", books.length === 1);
    books.forEach((book, index) => list.append(buildBook(book, index)));
  }

  function select(input) {
    if (window.__runningGuard && !window.__runningGuard()) return;
    selected = input;
    for (const item of document.querySelectorAll(".book")) {
      const isSelected = item.dataset.input === input;
      item.classList.toggle("selected", isSelected);
      item.setAttribute("aria-pressed", String(isSelected));
    }
    if (typeof window.__onBookSelected === "function") window.__onBookSelected(input);
  }

  return {
    /** Replace the shelf contents from the backend. */
    set(next) {
      books = next;
      if (selected && !books.some((b) => b.input === selected)) selected = null;
      // Keep a sensible default so the run card always names something.
      if (!selected && books.length) selected = books[0].input;
      render();
      if (selected && typeof window.__onBookSelected === "function") {
        window.__onBookSelected(selected);
      }
    },
    /** Add books and re-render from the backend's answer. */
    async add(inputs) {
      if (!inputs.length) return;
      books = await call("add_books", { inputs });
      selected = inputs[0];
      render();
      if (typeof window.__onBookSelected === "function") window.__onBookSelected(selected);
    },
    get selected() {
      return selected;
    },
    get books() {
      return books;
    },
    /** Drop a cached cover so a re-added book is re-read. */
    forget(input) {
      coverCache.delete(input);
    },
    render,
  };
})();
