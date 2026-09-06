let pathInput = document.getElementById("path-input");
let filterInput = document.getElementById("file-filter");
let refreshBtn = document.getElementById("refresh-btn");
let folderBtn = document.getElementById("folder-btn");
let treeEl = document.getElementById("file-tree");
let listEl = document.getElementById("file-list");
let subtitleEl = document.getElementById("files-subtitle");
let previewHeader = document.getElementById("preview-header");
let previewContent = document.getElementById("preview-content");

let currentDir = "/";
let homeDir = "";
let dirCache = new Map();
let expandedDirs = new Set();
let loadingDir = false;
let navToken = 0;
let selectedFile = null;
let searchResults = [];
let searchMode = false;
let showHidden = false;
let treeFull = false;
let listEntries = [];
let listIndex = -1;
let treeEntries = [];
let treeIndex = -1;
let activePane = "list";
let ws = null;
let gridZoom = 5;
let currentGridEl = null;
let gridItemCount = 0;
let gridEntries = [];
let gridIndex = -1;
let currentFileMime = "";
let appsCache = null;
let previewProps = false;
const GRID_MAX = 10;

const ICON_THEME = "breeze-dark";
const ICON_DIR = "icons/" + ICON_THEME + "/";
const ICON_FOLDER_SUBDIR = "places/96/";
const ICON_FILE_SUBDIR = "mimetypes/64/";
const ICON_ACTION_SUBDIR = "actions/24/";
const DEFAULT_FILE_ICON = "text-x-generic.svg";
const DEFAULT_BINARY_ICON = "application-octet-stream.svg";
const LOGGED_MISSING_ICONS = new Set();

const FOLDER_SPECIAL = {
  ".git": "folder-git",
  "node_modules": "folder-development",
  ".venv": "folder-development",
  "venv": "folder-development",
  "env": "folder-development",
  ".env": "folder-development",
  "target": "folder-development",
  "build": "folder-development",
  "dist": "folder-development",
  ".idea": "folder-development",
  ".vscode": "folder-development",
  "desktop": "folder-desktop",
  "documents": "folder-documents",
  "downloads": "folder-download",
  "pictures": "folder-pictures",
  "music": "folder-music",
  "videos": "folder-videos",
  "public": "folder-publicshare",
  "home": "user-home",
};

const EXT_ICONS = {
  toml: "application-toml",
  rs: "text-x-rust",
  yaml: "text-x-generic",
  yml: "text-x-generic",
  c: "text-x-csrc",
  h: "text-x-chdr",
  cpp: "text-x-c++src",
  cc: "text-x-c++src",
  hpp: "text-x-c++hdr",
  py: "text-x-python3",
  pyc: "text-x-python",
  js: "text-javascript",
  jsx: "text-javascript",
  ts: "text-javascript",
  tsx: "text-javascript",
  json: "application-json",
  jsonc: "application-json",
  json5: "application-json",
  xml: "application-xml",
  html: "text-html",
  htm: "text-html",
  css: "text-css",
  md: "text-markdown",
  markdown: "text-markdown",
  tex: "text-x-tex",
  sql: "text-x-sql",
  log: "text-x-log",
  sh: "text-x-script",
  bash: "text-x-script",
  zsh: "text-x-script",
  fish: "text-x-script",
  png: "image-x-generic",
  jpg: "image-x-generic",
  jpeg: "image-x-generic",
  gif: "image-x-generic",
  svg: "image-x-generic",
  webp: "image-x-generic",
  bmp: "image-x-generic",
  ico: "image-x-generic",
  avif: "image-x-generic",
  tiff: "image-x-generic",
  tif: "image-x-generic",
  psd: "image-x-generic",
  ai: "image-x-generic",
  eps: "image-x-generic",
  mp4: "video-x-generic",
  mkv: "video-x-generic",
  webm: "video-x-generic",
  mov: "video-x-generic",
  avi: "video-x-generic",
  m4v: "video-x-generic",
  mpg: "video-x-generic",
  mpeg: "video-x-generic",
  mp3: "audio-x-generic",
  flac: "audio-x-generic",
  ogg: "audio-x-generic",
  wav: "audio-x-generic",
  aac: "audio-x-generic",
  m4a: "audio-x-generic",
  wma: "audio-x-generic",
  mid: "audio-x-generic",
  pdf: "application-pdf",
  rtf: "application-rtf",
  zip: "application-x-archive",
  tar: "application-x-archive",
  gz: "application-x-archive",
  xz: "application-x-archive",
  bz2: "application-x-archive",
  "7z": "application-x-7z-compressed",
  rpm: "application-x-rpm",
  deb: "application-x-deb",
  iso: "package-x-generic",
  appimage: "package-x-generic",
  so: "application-x-sharedlib",
  o: "application-x-sharedlib",
  a: "application-x-sharedlib",
  ttf: "application-x-font-ttf",
  otf: "application-x-font-ttf",
  woff: "application-x-font-ttf",
  woff2: "application-x-font-ttf",
  bin: "application-x-executable",
  elf: "application-x-executable",
  exe: "application-x-executable",
  class: "application-octet-stream",
  jar: "application-octet-stream",
};

const NAME_ICONS = {
  "makefile": "text-x-makefile",
  "cmakelists.txt": "text-x-cmake",
  "cargo.toml": "application-toml",
  "cargo.lock": "application-toml",
  ".gitignore": "text-x-script",
  ".gitattributes": "text-x-script",
  ".dockerignore": "text-x-script",
  ".npmignore": "text-x-script",
  ".editorconfig": "text-x-script",
  ".gitmodules": "text-x-script",
  ".bashrc": "text-x-script",
  ".bash_profile": "text-x-script",
  ".profile": "text-x-script",
  ".zshrc": "text-x-script",
  ".zprofile": "text-x-script",
};

const IMAGE_EXTS = ["png","jpg","jpeg","gif","svg","webp","bmp","ico","avif","tiff","tif","psd","ai","eps"];

function isImageName(name) {
  const dot = name.lastIndexOf(".");
  const ext = dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
  return IMAGE_EXTS.indexOf(ext) !== -1;
}

/// MIME → icon stem fallback for types the extension map can't cover
/// (notably files with no extension, classified server-side by magic bytes).
const MIME_ICONS = {
  "application/x-executable": "application-x-executable",
  "application/x-sharedlib": "application-x-sharedlib",
  "text/x-script": "text-x-script",
  "application/pdf": "application-pdf",
  "application/zip": "application-x-archive",
  "application/gzip": "application-x-archive",
  "application/x-bzip2": "application-x-archive",
  "application/x-xz": "application-x-archive",
  "application/vnd.rar": "application-x-archive",
  "application/x-tar": "application-x-archive",
  "application/x-7z-compressed": "application-x-7z-compressed",
  "application/x-deb": "application-x-deb",
  "application/x-rpm": "application-x-rpm",
  "application/java-archive": "application-x-archive",
  "application/x-iso9660-image": "package-x-generic",
  "application/json": "application-json",
  "application/xml": "application-xml",
  "text/html": "text-html",
  "application/javascript": "text-javascript",
};

function iconForMime(mime) {
  if (!mime) return null;
  const exact = MIME_ICONS[mime];
  if (exact) return exact;
  if (mime.startsWith("image/")) return "image-x-generic";
  if (mime.startsWith("video/")) return "video-x-generic";
  if (mime.startsWith("audio/")) return "audio-x-generic";
  if (mime.startsWith("text/")) return "text-x-generic";
  return null;
}

function warnMissingIcon(entry, ext) {
  const key = ext + "\u0000" + (entry.mime || "");
  if (LOGGED_MISSING_ICONS.has(key)) return;
  LOGGED_MISSING_ICONS.add(key);
  console.warn(
    "no icon for \"" + entry.name +
    "\" (ext: " + (ext || "none") +
    ", mime: " + (entry.mime || "none") + ")"
  );
}

function iconFileForEntry(entry) {
  const base = entry.name;
  const low = base.toLowerCase();
  const nameIcon = NAME_ICONS[low] ||
    (low.startsWith("readme") ? "text-x-readme" : null) ||
    (low.startsWith("license") || low === "copying" || low.startsWith("copying") ? "text-x-copying" : null);
  if (entry.is_dir) {
    const special = FOLDER_SPECIAL[low];
    return ICON_DIR + ICON_FOLDER_SUBDIR + (special || "folder") + ".svg";
  }
  const dot = low.lastIndexOf(".");
  let ext = dot < 0 ? "" : low.slice(dot + 1);
  if (dot === 0 && ext.length > 1) ext = low.slice(1);
  const icon = nameIcon || EXT_ICONS[ext] || iconForMime(entry.mime) || DEFAULT_FILE_ICON;
  if (icon === DEFAULT_FILE_ICON) warnMissingIcon(entry, ext);
  return ICON_DIR + ICON_FILE_SUBDIR + icon + ".svg";
}

function onIconError() {
  if (this.dataset.fallbackIcon) return;
  this.dataset.fallbackIcon = "1";
  if (this.src.indexOf(ICON_FOLDER_SUBDIR) !== -1) {
    this.src = ICON_DIR + ICON_FOLDER_SUBDIR + "folder.svg";
  } else {
    this.src = ICON_DIR + ICON_FILE_SUBDIR + DEFAULT_FILE_ICON;
  }
}

function iconEl(entry, cls) {
  const img = document.createElement("img");
  img.className = cls || "tree-icon";
  img.src = iconFileForEntry(entry);
  img.alt = "";
  img.draggable = false;
  img.addEventListener("error", onIconError);
  return img;
}

function setSubtitle(text) {
  subtitleEl.textContent = text;
}

async function fetchInfo() {
  try {
    const res = await fetch("/info");
    const info = await res.json();
    homeDir = info.home || "";
    return info.root;
  } catch {
    return null;
  }
}

function parentOf(path) {
  const p = path.replace(/\/+$/, "");
  if (!p) return "/";
  const slash = p.lastIndexOf("/");
  if (slash <= 0) return "/";
  return p.slice(0, slash);
}

function visibleEntry(entry) {
  return showHidden || !entry.name.startsWith(".");
}

function refreshView() {
  if (searchMode) {
    renderFileList();
    return;
  }
  renderFileList();
  renderFolderTree();
  const visible = (dirCache.get(currentDir) || []).filter(visibleEntry);
  const files = visible.filter((e) => !e.is_dir).length;
  setSubtitle(visible.length ? files + " files" : "(empty)");
  if (!selectedFile) showFolderView(currentDir);
}

function updatePathBar() {
  pathInput.value = currentDir;
}

async function getDirEntries(dir) {
  let entries = dirCache.get(dir);
  if (!entries) {
    const res = await fetch("/filetree?path=" + encodeURIComponent(dir));
    if (!res.ok) return null;
    entries = await res.json();
    dirCache.set(dir, entries);
  }
  return entries;
}

async function loadDir(dir) {
  if (loadingDir) return;
  loadingDir = true;
  const token = navToken;
  const prev = currentDir;
  currentDir = dir;
  updatePathBar();
  try {
    const entries = await getDirEntries(dir);
    if (token !== navToken) return;
    if (!entries) {
      currentDir = prev;
      setSubtitle("no such directory: " + dir);
      renderFileList();
      renderFolderTree();
      updatePathBar();
      return;
    }
    if (token !== navToken) return;
    renderFileList();
    await revealTree(dir);
    if (token !== navToken) return;
    renderFolderTree();
    if (!searchMode) {
      const visible = entries.filter(visibleEntry);
      const files = visible.filter((e) => !e.is_dir).length;
      setSubtitle(visible.length ? files + " files" : "(empty)");
      if (!selectedFile) showFolderView(currentDir);
    }
  } catch {
    if (token !== navToken) return;
    currentDir = prev;
    setSubtitle("failed to list directory");
    updatePathBar();
  } finally {
    if (token === navToken) loadingDir = false;
  }
}

function navigateDir(dir) {
  navToken++;
  loadingDir = false;
  if (searchMode) {
    searchMode = false;
    filterInput.value = "";
  }
  selectedFile = null;
  loadDir(dir);
  sendWatch();
}

function navigateToTypedPath() {
  const raw = pathInput.value.trim();
  if (!raw) return;
  let dir;
  if (raw === "~" || raw === "~/") {
    dir = homeDir || currentDir;
  } else if (raw.startsWith("~/")) {
    dir = homeDir + raw.slice(1);
  } else if (raw.startsWith("/")) {
    dir = raw;
  } else {
    dir = currentDir + "/" + raw;
  }
  dir = dir.replace(/\/+/g, "/").replace(/\/+$/, "");
  if (!dir) dir = "/";
  navigateDir(dir);
}

async function fetchSearch(query) {
  const token = ++navToken;
  try {
    const res = await fetch(
      "/filesearch?q=" + encodeURIComponent(query) +
      "&path=" + encodeURIComponent(currentDir)
    );
    if (token !== navToken) return;
    searchResults = await res.json();
    if (token !== navToken) return;
    searchMode = true;
    renderFileList();
  } catch {
    if (token === navToken) setSubtitle("search failed");
  }
}

function gridIconEl(entry) {
  const img = document.createElement("img");
  img.className = "grid-icon";
  img.loading = "lazy";
  img.draggable = false;
  img.alt = "";
  if (!entry.is_dir && isImageName(entry.name)) {
    img.src = "/filecontent?path=" + encodeURIComponent(entry.path) + "&raw=true";
  } else {
    img.src = iconFileForEntry(entry);
    img.addEventListener("error", onIconError);
  }
  return img;
}

function applyGridZoom() {
  if (!currentGridEl) return;
  currentGridEl.style.setProperty("--icon", (gridZoom * 16) + "px");
  currentGridEl.style.setProperty("--name", (11 + gridZoom) + "px");
  currentGridEl.style.setProperty("--gap", (8 + gridZoom) + "px");
}

function showFolderView(dir) {
  const entries = (dirCache.get(dir) || []).filter(visibleEntry);
  gridItemCount = entries.length;
  currentGridEl = null;
  gridEntries = [];
  gridIndex = -1;
  previewHeader.textContent = "";
  previewContent.textContent = "";

  const title = document.createElement("span");
  title.className = "filename";
  title.textContent = dir === "/" ? "/" : dir;
  const spacer = document.createElement("span");
  spacer.className = "header-spacer";
  const meta = document.createElement("span");
  meta.className = "muted";
  meta.textContent = gridItemCount + " items · " + (gridZoom * 16) + "px";
  previewHeader.append(title, spacer, meta);

  if (!entries.length) {
    const p = document.createElement("p");
    p.className = "muted";
    p.textContent = "(empty)";
    previewContent.appendChild(p);
    return;
  }

  const grid = document.createElement("div");
  grid.className = "icon-grid";
  grid.title = "Ctrl + mouse wheel to zoom";
  currentGridEl = grid;
  applyGridZoom();

  const sorted = [...entries].sort((a, b) =>
    (b.is_dir - a.is_dir) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
  );
  gridEntries = sorted;
  for (const entry of sorted) {
    const item = document.createElement("div");
    item.className = "grid-item";
    const icon = gridIconEl(entry);
    const name = document.createElement("span");
    name.className = "grid-name";
    name.textContent = entry.name;
    name.title = entry.path;
    item.append(icon, name);
    item.addEventListener("click", () => selectEntry(entry));
    grid.appendChild(item);
  }
  previewContent.appendChild(grid);
  applyGridCursor();
}

async function revealTree(dir) {
  const chain = ["/"];
  const parts = [];
  let d = dir;
  while (d && d !== "/") {
    parts.push(d);
    d = parentOf(d);
  }
  for (let i = parts.length - 1; i >= 0; i--) chain.push(parts[i]);
  for (const p of chain) {
    if (!dirCache.has(p)) await getDirEntries(p);
    expandedDirs.add(p);
  }
}

async function toggleExpand(dir) {
  if (expandedDirs.has(dir)) {
    expandedDirs.delete(dir);
    for (const d of [...expandedDirs]) {
      if (d.startsWith(dir + "/")) expandedDirs.delete(d);
    }
    renderFolderTree(dir);
    return;
  }
  await getDirEntries(dir);
  expandedDirs.add(dir);
  renderFolderTree(dir);
}

async function renderTreeNode(dir, depth) {
  const entries = dirCache.get(dir);
  if (!entries) return;
  const folders = entries
    .filter((e) => visibleEntry(e) && e.is_dir)
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  for (const folder of folders) {
    const row = document.createElement("div");
    row.className = "tree-item" + (folder.path === currentDir ? " current" : "");
    row.style.paddingLeft = (8 + depth * 16) + "px";
    const toggle = document.createElement("span");
    toggle.className = "tree-toggle";
    toggle.textContent = expandedDirs.has(folder.path) ? "▾" : "▸";
    const icon = iconEl(folder);
    const name = document.createElement("span");
    name.textContent = folder.name;
    name.title = folder.path;
    row.append(toggle, icon, name);
    row.addEventListener("click", (e) => {
      if (e.target.closest(".tree-toggle")) {
        toggleExpand(folder.path);
        return;
      }
      navigateDir(folder.path);
    });
    treeEl.appendChild(row);
    treeEntries.push(folder);
    if (folder.path === currentDir) treeIndex = treeEntries.length - 1;
    if (expandedDirs.has(folder.path)) {
      await renderTreeNode(folder.path, depth + 1);
    }
  }
}

async function renderTreeHomeRow() {
  const row = document.createElement("div");
  row.className = "tree-item";
  row.title = treeFull ? "collapse back to home view" : "expand to full filesystem view";
  const toggle = document.createElement("span");
  toggle.className = "tree-toggle";
  toggle.textContent = treeFull ? "▾" : "▸";
  const img = document.createElement("img");
  img.className = "tree-icon";
  img.src = ICON_DIR + ICON_ACTION_SUBDIR + "go-up.svg";
  img.alt = "";
  img.draggable = false;
  img.addEventListener("error", onIconError);
  const name = document.createElement("span");
  name.textContent = "..";
  row.append(toggle, img, name);
  row.addEventListener("click", () => toggleTreeFull());
  treeEl.appendChild(row);
  treeEntries.push({ name: "..", path: null, is_dir: true, pseudo: true });
}

async function toggleTreeFull() {
  treeFull = !treeFull;
  if (homeDir && !dirCache.has(homeDir)) await getDirEntries(homeDir);
  if (!dirCache.has("/")) await getDirEntries("/");
  renderFolderTree();
}

async function renderFolderTree(focusPath) {
  treeEl.textContent = "";
  treeEntries = [];
  treeIndex = -1;
  await renderTreeHomeRow();
  if (homeDir && !dirCache.has(homeDir)) {
    await getDirEntries(homeDir);
  }
  const root = homeDir || "/";
  if (treeFull) {
    if (dirCache.has("/")) await renderTreeNode("/", 0);
  } else if (dirCache.has(root)) {
    await renderTreeNode(root, 0);
  }
  const focusTarget = (focusPath && treeEntries.some((f) => f.path === focusPath)) ? focusPath : currentDir;
  treeIndex = treeEntries.findIndex((f) => f.path === focusTarget);
  applyTreeCursor();
  if (treeIndex >= 0) {
    const row = treeEl.children[treeIndex];
    if (row) row.scrollIntoView({ block: "center" });
  }
}

function renderFileList() {
  listEl.textContent = "";
  if (searchMode) {
    listEntries = searchResults.filter(visibleEntry);
    setSubtitle(listEntries.length ? listEntries.length + " matches" : "No matches");
    for (const entry of listEntries) {
      const row = document.createElement("div");
      row.className = "tree-item" + (selectedFile === entry.path ? " selected" : "");
      const icon = iconEl(entry);
      const name = document.createElement("span");
      name.textContent = entry.path;
      name.title = entry.path;
      row.append(icon, name);
      row.addEventListener("click", () => selectEntry(entry));
      listEl.appendChild(row);
    }
  } else {
    const entries = dirCache.get(currentDir) || [];
    const files = entries
      .filter((e) => visibleEntry(e) && !e.is_dir)
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
    listEntries = files;
    for (const entry of files) {
      const row = document.createElement("div");
      row.className = "tree-item" + (selectedFile === entry.path ? " selected" : "");
      const icon = iconEl(entry);
      const name = document.createElement("span");
      name.textContent = entry.name;
      name.title = entry.path;
      row.append(icon, name);
      row.addEventListener("click", () => selectEntry(entry));
      listEl.appendChild(row);
    }
  }
  listIndex = listEntries.findIndex((e) => e.path === selectedFile);
}

function applyTreeCursor() {
  for (let i = 0; i < treeEl.children.length; i++) {
    treeEl.children[i].classList.toggle("focused", i === treeIndex);
  }
}

function setTreeCursor(i) {
  if (i < 0 || i >= treeEntries.length) return;
  treeIndex = i;
  applyTreeCursor();
  const row = treeEl.children[i];
  if (row) row.scrollIntoView({ block: "nearest" });
}

function focusListEntry(i) {
  if (i < 0 || i >= listEntries.length) return;
  listIndex = i;
  const rows = listEl.children;
  for (let j = 0; j < rows.length; j++) rows[j].classList.toggle("selected", j === i);
  if (rows[i]) rows[i].scrollIntoView({ block: "nearest" });
  const entry = listEntries[i];
  if (entry && !entry.is_dir) selectFile(entry.path);
}

function setActivePane(pane) {
  activePane = pane;
  for (const el of [document.querySelector(".file-tree-pane"), document.querySelector(".file-list-pane"), document.querySelector(".file-preview")]) {
    el.classList.toggle("pane-active", el.matches(".file-tree-pane") && pane === "tree" ||
      el.matches(".file-list-pane") && pane === "list" ||
      el.matches(".file-preview") && pane === "preview");
  }
}

async function selectEntry(entry) {
  selectedFile = entry.path;
  if (entry.is_dir) {
    navigateDir(entry.path);
    return;
  }
  renderFileList();
  await selectFile(entry.path);
}

async function selectFile(path) {
  selectedFile = path;
  renderFileList();
  currentGridEl = null;
  gridEntries = [];
  gridIndex = -1;
  previewHeader.textContent = "";
  previewContent.textContent = "";
  currentFileMime = "";
  try {
    const res = await fetch("/filecontent?path=" + encodeURIComponent(path));
    const data = await res.json();
    currentFileMime = data.mime || "";
    const slash = path.lastIndexOf("/");
    const base = slash < 0 ? path : path.slice(slash + 1);
    const filename = document.createElement("span");
    filename.className = "filename";
    filename.textContent = base;
    filename.title = path;
    previewHeader.appendChild(filename);

    const spacer = document.createElement("span");
    spacer.className = "header-spacer";
    previewHeader.appendChild(spacer);

    if (!data.error) {
      const editBtn = document.createElement("button");
      editBtn.type = "button";
      editBtn.className = "action-btn open-with";
      editBtn.textContent = "\u270e";
      editBtn.title = "Open with the app last used for this file type";
      editBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openLastUsed(editBtn, path);
      });
      previewHeader.appendChild(editBtn);

      const caretBtn = document.createElement("button");
      caretBtn.type = "button";
      caretBtn.className = "action-btn caret";
      caretBtn.textContent = "\u25be";
      caretBtn.title = "Choose an app\u2026";
      caretBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openWithMenu(caretBtn, path);
      });
      previewHeader.appendChild(caretBtn);
    }

    const menuBtn = document.createElement("button");
    menuBtn.type = "button";
    menuBtn.className = "action-btn";
    menuBtn.textContent = "\u22ef";
    menuBtn.title = "Actions";
    menuBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      if (dropdownEl.style.display === "block") {
        hideDropdown();
        return;
      }
      showDropdown(menuBtn, (menu) => {
        const props = document.createElement("button");
        props.type = "button";
        props.textContent = previewProps ? "Hide properties" : "Properties";
        props.addEventListener("click", () => toggleProperties(path));
        const rename = document.createElement("button");
        rename.type = "button";
        rename.textContent = "Rename";
        rename.addEventListener("click", () => renameFile(path));
        const del = document.createElement("button");
        del.type = "button";
        del.textContent = "Delete";
        del.className = "danger";
        del.addEventListener("click", () => deleteFile(path));
        menu.append(props, rename, del);
      });
    });
    previewHeader.appendChild(menuBtn);

    const content = document.createElement("div");
    if (data.error) {
      content.className = "preview-error";
      content.textContent = data.error;
    } else if (data.is_image) {
      const img = document.createElement("img");
      img.src = "/filecontent?path=" + encodeURIComponent(path) + "&raw=true";
      content.appendChild(img);
    } else if (data.is_binary) {
      content.className = "preview-binary";
      content.textContent = "Binary file (" + data.size + " bytes)";
    } else {
      const table = document.createElement("table");
      table.className = "text-view";
      const lines = data.content.split("\n");
      if (lines.length && lines[lines.length - 1] === "") lines.pop();
      for (let i = 0; i < lines.length; i++) {
        const row = document.createElement("tr");
        const num = document.createElement("td");
        num.className = "line-num";
        num.textContent = String(i + 1);
        const textCell = document.createElement("td");
        textCell.className = "line-text";
        textCell.textContent = lines[i];
        row.append(num, textCell);
        table.appendChild(row);
      }
      content.appendChild(table);
    }
    previewContent.appendChild(content);
    if (previewProps) await renderProperties(path);
  } catch (e) {
    setSubtitle("failed to load preview");
  }
}

async function getApps() {
  if (appsCache) return appsCache;
  try {
    const res = await fetch("/apps");
    appsCache = res.ok ? await res.json() : [];
  } catch {
    appsCache = [];
  }
  return appsCache;
}

function appMatchesMime(app, mime) {
  if (!mime || !app.mime_types) return false;
  return app.mime_types.some((m) =>
    m === mime ||
    (m.endsWith("/*") && mime.startsWith(m.slice(0, m.length - 1)))
  );
}

async function openLastUsed(anchor, path) {
  const apps = await getApps();
  let id = null;
  try {
    const res = await fetch("/defaultapp?mime=" + encodeURIComponent(currentFileMime));
    if (res.ok) id = (await res.json()).id;
  } catch {}
  const app = id && apps.find((a) => a.id === id);
  if (app) {
    openWith(app, path);
  } else {
    openWithMenu(anchor, path);
  }
}

async function openWithMenu(anchor, path) {
  const apps = await getApps();
    showDropdown(anchor, (menu) => {
        const matches = apps.filter((a) => appMatchesMime(a, currentFileMime));
        if (!matches.length) {
            const none = document.createElement("button");
            none.textContent = apps.length ? "no app matches this file type" : "no apps found";
            none.disabled = true;
            menu.appendChild(none);
            return;
        }
        appendAppGroup(menu, matches, "", path);
    }, "app-list");
}

async function openFolderLastUsed(anchor, path) {
  const apps = await getApps();
  let id = null;
  try {
    const res = await fetch("/defaultapp?mime=" + encodeURIComponent("inode/directory"));
    if (res.ok) id = (await res.json()).id;
  } catch {}
  const app = id && apps.find((a) => a.id === id);
  if (app) {
    openWith(app, path);
  } else {
    openFolderMenu(anchor, path);
  }
}

async function openFolderMenu(anchor, path) {
  const apps = await getApps();
  showDropdown(anchor, (menu) => {
    const matches = apps.filter((a) => appMatchesMime(a, "inode/directory"));
    if (!matches.length) {
      const none = document.createElement("button");
      none.textContent = apps.length ? "no app matches folder" : "no apps found";
      none.disabled = true;
      menu.appendChild(none);
      return;
    }
    appendAppGroup(menu, matches, "", path);
  }, "app-list");
}

function appendAppGroup(menu, apps, label, path) {
  if (!apps.length) return;
  if (label) {
    const labelEl = document.createElement("div");
    labelEl.className = "dropdown-label";
    labelEl.textContent = label;
    menu.appendChild(labelEl);
  }
  for (const app of apps) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = app.name;
    btn.title = app.id;
    btn.addEventListener("click", () => openWith(app, path));
    menu.appendChild(btn);
  }
}

async function openWith(app, path) {
  hideDropdown();
  const res = await fetch("/open", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id: app.id, path }),
  });
  const data = await res.json();
  setSubtitle(res.ok && data && data.ok
    ? "opened with " + app.name
    : (res.status === 404 ? "cannot open: no such file" : "failed to open with " + app.name));
}

async function toggleProperties(path) {
  previewProps = !previewProps;
  const el = document.getElementById("file-properties");
  if (!previewProps) {
    el.hidden = true;
    el.textContent = "";
    return;
  }
  el.hidden = false;
  await renderProperties(path);
}

async function renderProperties(path) {
  const el = document.getElementById("file-properties");
  if (!previewProps || !selectedFile) return;
  let info = null;
  try {
    const res = await fetch("/fileinfo?path=" + encodeURIComponent(path));
    if (!res.ok) {
      el.textContent = "failed to load properties";
      return;
    }
    info = await res.json();
  } catch {
    el.textContent = "failed to load properties";
    return;
  }
  el.textContent = "";
  const rows = [
    ["Name", info.name],
    ["Path", info.path],
    ["Type", info.is_dir ? "Folder" : (info.mime || "Unknown")],
    ["Size", info.is_dir ? "\u2014" : fmtSize(info.size)],
    ["Modified", new Date(info.modified * 1000).toLocaleString()],
    ["Permissions", fmtPerms(info.mode)],
  ];
  if (info.media) {
    if (info.media.width && info.media.height) {
      rows.push(["Dimensions", info.media.width + " \u00d7 " + info.media.height + " px"]);
    }
    if (typeof info.media.duration_secs === "number") {
      rows.push(["Duration", fmtDuration(info.media.duration_secs)]);
    }
  }
  for (const [label, value] of rows) {
    const labelEl = document.createElement("div");
    labelEl.className = "prop-label";
    labelEl.textContent = label;
    const valueEl = document.createElement("div");
    valueEl.className = "prop-value";
    valueEl.textContent = value;
    el.append(labelEl, valueEl);
  }
}

function fmtSize(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return (i === 0 ? String(v) : v.toFixed(1)) + " " + units[i];
}

function fmtPerms(mode) {
  const bits = mode & 0o777;
  let s = "";
  for (let i = 6; i >= 0; i -= 3) {
    const m = (bits >> i) & 7;
    s += (m & 4 ? "r" : "-") + (m & 2 ? "w" : "-") + (m & 1 ? "x" : "-");
  }
  return "0" + bits.toString(8) + " \u2022 " + s;
}

function fmtDuration(secs) {
  if (secs < 60) return (secs < 10 ? secs.toFixed(1) : Math.round(secs)) + "s";
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return m + ":" + String(s).padStart(2, "0");
}

function resetPreview() {
  selectedFile = null;
  currentGridEl = null;
  gridEntries = [];
  gridIndex = -1;
  previewProps = false;
  const propsEl = document.getElementById("file-properties");
  propsEl.hidden = true;
  propsEl.textContent = "";
  if (searchMode) {
    previewHeader.textContent = "";
    previewContent.textContent = "";
    return;
  }
  showFolderView(currentDir);
}

filterInput.addEventListener("input", () => {
  const q = filterInput.value.trim();
  if (!q) {
    searchMode = false;
    renderFileList();
    if (!selectedFile) showFolderView(currentDir);
    return;
  }
  fetchSearch(q);
});

refreshBtn.addEventListener("click", () => {
  navToken++;
  loadingDir = false;
  dirCache.clear();
  expandedDirs.clear();
  loadDir(currentDir);
  if (selectedFile) selectFile(selectedFile);
});

folderBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  openFolderLastUsed(folderBtn, currentDir);
});

pathInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    pathInput.blur();
    navigateToTypedPath();
  } else if (e.key === "Escape") {
    e.preventDefault();
    updatePathBar();
    pathInput.blur();
  }
});

pathInput.addEventListener("focus", () => {
  pathInput.select();
});

previewContent.addEventListener("wheel", (e) => {
  if (!e.ctrlKey || !currentGridEl) return;
  e.preventDefault();
  gridZoom = Math.max(1, Math.min(GRID_MAX, gridZoom + (e.deltaY < 0 ? 1 : -1)));
  applyGridZoom();
  const meta = previewHeader.querySelector(".muted");
  if (meta) meta.textContent = gridItemCount + " items · " + (gridZoom * 16) + "px";
}, { passive: false });

let dropdownEl = document.getElementById("dropdown");
let menuAnchor = null;

function showDropdown(anchor, buildFn, className = "") {
  hideDropdown();
  menuAnchor = anchor;
  dropdownEl.textContent = "";
  const menu = document.createElement("div");
  menu.className = "dropdown-menu" + (className ? " " + className : "");
  buildFn(menu);
  if (className === "app-list") {
    const itemCount = menu.querySelectorAll("button").length;
    const cols = Math.min(4, Math.max(1, Math.ceil(itemCount / 12)));
    menu.style.setProperty("--cols", cols);
  }
  dropdownEl.appendChild(menu);
  dropdownEl.style.display = "block";
  positionDropdown(anchor);
  document.addEventListener("click", onDocumentClick, true);
  window.addEventListener("scroll", onScroll, true);
}

function positionDropdown(anchor) {
  const r = anchor.getBoundingClientRect();
  const w = dropdownEl.getBoundingClientRect();
  dropdownEl.style.left = Math.max(4, Math.min(r.left, window.innerWidth - w.width - 4)) + "px";
  dropdownEl.style.top = r.bottom + 2 + "px";
}

function onDocumentClick(e) {
  if (!dropdownEl.contains(e.target)) hideDropdown();
}

function onScroll() {
  if (menuAnchor) positionDropdown(menuAnchor);
}

function hideDropdown() {
  if (!menuAnchor) return;
  document.removeEventListener("click", onDocumentClick, true);
  window.removeEventListener("scroll", onScroll, true);
  dropdownEl.style.display = "none";
  dropdownEl.textContent = "";
  menuAnchor = null;
}

async function renameFile(path) {
  const slash = path.lastIndexOf("/");
  const base = slash < 0 ? path : path.slice(slash + 1);
  const dir = parentOf(path);
  const target = prompt("Rename \"" + base + "\" to:", base);
  if (!target || target === base || target.includes("/")) return;
  const newPath = (dir === "/" ? "/" : dir + "/") + target;
  hideDropdown();
  const res = await fetch("/rename", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ path, to: newPath }),
  });
  const data = await res.json();
  if (!res.ok || !data.ok) {
    setSubtitle("rename failed");
    return;
  }
  selectedFile = newPath;
  await onFileChanged(newPath);
  selectFile(newPath);
}

async function deleteFile(path) {
  if (!confirm("Delete \"" + path + "\"?")) return;
  hideDropdown();
  const res = await fetch("/delete", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ path }),
  });
  const data = await res.json();
  if (!res.ok || !data.ok) {
    setSubtitle(res.status === 409 ? "could not delete (in use?)" : "delete failed");
    return;
  }
  selectedFile = null;
  await onFileChanged(path);
  resetPreview();
}

let changesTimer = null;
let pendingPath = null;

function scheduleChanged(path) {
  pendingPath = path;
  if (changesTimer) return;
  changesTimer = setTimeout(() => {
    changesTimer = null;
    const p = pendingPath;
    pendingPath = null;
    if (p != null) onFileChanged(p);
  }, 150);
}

async function onFileChanged(path) {
  if (searchMode) {
    const q = filterInput.value.trim();
    if (q) fetchSearch(q);
    return;
  }
  navToken++;
  const token = navToken;
  loadingDir = false;
  dirCache.clear();
  const dirs = [currentDir, ...Array.from(expandedDirs)];
  for (const d of [...new Set(dirs)]) {
    if (token !== navToken) return;
    await getDirEntries(d);
  }
  renderFileList();
  renderFolderTree();
  if (selectedFile &&
      (selectedFile === path ||
        selectedFile.startsWith(path + "/") ||
        path.startsWith(selectedFile + "/"))) {
    selectFile(selectedFile);
  } else if (!selectedFile) {
    showFolderView(currentDir);
  }
}

function sendWatch() {
  if (ws && ws.readyState === WebSocket.OPEN) {
    try {
      ws.send(JSON.stringify({ type: "watch", path: currentDir }));
    } catch {}
  }
}

function connectSocket() {
  const proto = location.protocol === "https:" ? "wss://" : "ws://";
  ws = new WebSocket(proto + location.host + "/ws");
  ws.onopen = sendWatch;
  ws.onmessage = (ev) => {
    let hint;
    try {
      hint = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (hint && hint.type === "changed" && typeof hint.path === "string") {
      scheduleChanged(hint.path);
    }
  };
  ws.onclose = () => setTimeout(connectSocket, 2000);
}

function cyclePane(step) {
  const order = ["tree", "list", "preview"];
  const idx = order.indexOf(activePane);
  setActivePane(order[(idx + step + order.length) % order.length]);
}

function applyGridCursor() {
  if (!currentGridEl) return;
  for (let i = 0; i < currentGridEl.children.length; i++) {
    currentGridEl.children[i].classList.toggle("focused", i === gridIndex);
  }
}

function setGridCursor(i) {
  if (i < 0 || i >= gridEntries.length) return;
  gridIndex = i;
  applyGridCursor();
  const item = currentGridEl && currentGridEl.children[i];
  if (item) item.scrollIntoView({ block: "nearest" });
}

function moveCursor(step) {
  if (activePane === "tree") {
    if (!treeEntries.length) return;
    let i = treeIndex < 0 ? (step === 1 ? 0 : treeEntries.length - 1) : treeIndex + step;
    if (i >= treeEntries.length) i = treeEntries.length - 1;
    if (i < 0) i = 0;
    setTreeCursor(i);
    return;
  }
  if (activePane === "list") {
    if (!listEntries.length) return;
    let i = listIndex < 0 ? (step === 1 ? 0 : listEntries.length - 1) : listIndex + step;
    if (i >= listEntries.length) i = listEntries.length - 1;
    if (i < 0) i = 0;
    focusListEntry(i);
    return;
  }
  if (activePane === "preview") {
    previewContent.scrollBy(0, 40 * step);
  }
}

function enterFolder() {
  if (activePane === "tree") {
    if (treeIndex < 0) return;
    const folder = treeEntries[treeIndex];
    if (!folder) return;
    if (folder.pseudo) {
      toggleTreeFull();
      return;
    }
    if (folder.path === currentDir) {
      setActivePane("list");
      return;
    }
    navigateDir(folder.path);
    return;
  }
  if (activePane === "list") {
    setActivePane("preview");
    return;
  }
  if (activePane === "preview") {
    if (gridIndex < 0) return;
    const entry = gridEntries[gridIndex];
    if (entry && entry.is_dir) navigateDir(entry.path);
  }
}

function goLeft() {
  if (activePane === "tree" && treeIndex >= 0) {
    const folder = treeEntries[treeIndex];
    if (folder && folder.pseudo) {
      if (treeFull) toggleTreeFull();
      return;
    }
    if (folder && expandedDirs.has(folder.path)) {
      expandedDirs.delete(folder.path);
      for (const d of [...expandedDirs]) {
        if (d.startsWith(folder.path + "/")) expandedDirs.delete(d);
      }
      renderFolderTree(folder.path);
      return;
    }
  }
  if (activePane === "list") {
    setActivePane("tree");
    return;
  }
  if (activePane === "preview") {
    setActivePane("list");
    return;
  }
  if (currentDir === "/") return;
  navigateDir(parentOf(currentDir));
}

async function boot() {
  document.addEventListener("keydown", (e) => {
    const tag = document.activeElement && document.activeElement.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    if (e.key === "Tab") {
      e.preventDefault();
      cyclePane(e.shiftKey ? -1 : 1);
      return;
    }
    if (e.ctrlKey && e.key === "ArrowLeft") {
      e.preventDefault();
      cyclePane(-1);
      return;
    }
    if (e.ctrlKey && e.key === "ArrowRight") {
      e.preventDefault();
      cyclePane(1);
      return;
    }
    if (e.ctrlKey && e.key.toLowerCase() === "h") {
      e.preventDefault();
      showHidden = !showHidden;
      refreshView();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "ArrowRight") {
      e.preventDefault();
      enterFolder();
      return;
    }
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      goLeft();
      return;
    }
    if (e.key === "PageUp" || e.key === "PageDown") {
      if (activePane === "preview") {
        e.preventDefault();
        previewContent.scrollBy(0, e.key === "PageDown" ? previewContent.clientHeight : -previewContent.clientHeight);
      }
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    moveCursor(e.key === "ArrowDown" ? 1 : -1);
  });

  document.querySelector(".file-tree-pane").addEventListener("click", () => setActivePane("tree"));
  document.querySelector(".file-list-pane").addEventListener("click", () => setActivePane("list"));
  document.querySelector(".file-preview").addEventListener("click", () => setActivePane("preview"));

  setActivePane("tree");

  const root = await fetchInfo();
  const startDir = new URLSearchParams(location.search).get("dir");
  const initial = startDir || root;
  if (initial) {
    // Grit opens folio with ?dir=<repo> as a per-repo start point. When the
    // target sits outside home, use the full-filesystem tree so the repo is
    // revealed in the folder tree instead of being unreachable from home.
    if (!homeDir || !(initial === homeDir || initial.startsWith(homeDir + "/"))) {
      treeFull = true;
    }
    await loadDir(initial);
  } else {
    currentDir = "/";
    setSubtitle("failed to reach the server");
    updatePathBar();
    renderFileList();
    showFolderView("/");
  }
  connectSocket();
}

boot();