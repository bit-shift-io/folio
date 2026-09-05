let pathInput = document.getElementById("path-input");
let filterInput = document.getElementById("file-filter");
let refreshBtn = document.getElementById("refresh-btn");
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
let listEntries = [];
let listIndex = -1;
let treeEntries = [];
let treeIndex = -1;
let activePane = "list";
let ws = null;
let gridZoom = 5;
let currentGridEl = null;
let gridItemCount = 0;
const GRID_MAX = 10;

const ICON_THEME = "breeze-dark";
const ICON_DIR = "icons/" + ICON_THEME + "/";
const ICON_FOLDER_SUBDIR = "places/96/";
const ICON_FILE_SUBDIR = "mimetypes/64/";
const ICON_ACTION_SUBDIR = "actions/24/";
const DEFAULT_FILE_ICON = "text-x-generic.svg";
const DEFAULT_BINARY_ICON = "application-octet-stream.svg";

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
  "text/x-script": "text-x-script",
  "application/pdf": "application-pdf",
  "application/zip": "application-x-archive",
  "application/gzip": "application-x-archive",
  "application/x-bzip2": "application-x-archive",
  "application/vnd.rar": "application-x-archive",
  "application/x-tar": "application-x-archive",
  "application/x-7z-compressed": "application-x-7z-compressed",
  "application/x-deb": "application-x-deb",
  "application/x-rpm": "application-x-rpm",
};

function iconForMime(mime) {
  if (!mime) return null;
  if (mime.startsWith("image/")) return "image-x-generic";
  if (mime.startsWith("video/")) return "video-x-generic";
  if (mime.startsWith("audio/")) return "audio-x-generic";
  if (mime.startsWith("text/")) return "text-x-generic";
  return MIME_ICONS[mime] || null;
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
  return ICON_DIR + ICON_FILE_SUBDIR + icon + ".svg";
}

function iconEl(entry, cls) {
  const img = document.createElement("img");
  img.className = cls || "tree-icon";
  img.src = iconFileForEntry(entry);
  img.alt = "";
  img.draggable = false;
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
      const files = entries.filter((e) => !e.is_dir).length;
      setSubtitle(entries.length ? files + " files" : "(empty)");
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
  const entries = (dirCache.get(dir) || []).slice();
  gridItemCount = entries.length;
  currentGridEl = null;
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
    renderFolderTree();
    return;
  }
  await getDirEntries(dir);
  expandedDirs.add(dir);
  renderFolderTree();
}

async function renderTreeNode(dir, depth) {
  const entries = dirCache.get(dir);
  if (!entries) return;
  const folders = entries
    .filter((e) => e.is_dir)
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

async function renderFolderTree() {
  treeEl.textContent = "";
  treeEntries = [];
  treeIndex = -1;
  if (dirCache.has("/")) {
    await renderTreeNode("/", 0);
  }
  const current = treeEl.querySelector(".current");
  if (current) current.scrollIntoView({ block: "center" });
}

function renderFileList() {
  listEl.textContent = "";
  if (searchMode) {
    setSubtitle(searchResults.length ? searchResults.length + " matches" : "No matches");
    listEntries = searchResults;
    for (const entry of searchResults) {
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
      .filter((e) => !e.is_dir)
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

function focusListEntry(i) {
  if (i < 0 || i >= listEntries.length) return;
  listIndex = i;
  const rows = listEl.children;
  if (rows[i]) {
    rows[i].scrollIntoView({ block: "nearest" });
  }
  selectEntry(listEntries[i]);
}

function focusTreeEntry(i) {
  if (i < 0 || i >= treeEntries.length) return;
  treeIndex = i;
  const rows = treeEl.children;
  if (rows[i]) {
    rows[i].scrollIntoView({ block: "center" });
  }
  navigateDir(treeEntries[i].path);
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
  previewHeader.textContent = "";
  previewContent.textContent = "";
  try {
    const res = await fetch("/filecontent?path=" + encodeURIComponent(path));
    const data = await res.json();
    const filename = document.createElement("span");
    filename.className = "filename";
    filename.textContent = path;
    previewHeader.appendChild(filename);

    const spacer = document.createElement("span");
    spacer.className = "header-spacer";
    const menuBtn = document.createElement("button");
    menuBtn.type = "button";
    menuBtn.className = "header-actions-btn";
    menuBtn.textContent = "…";
    menuBtn.title = "Actions";
    menuBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      if (dropdownEl.style.display === "block") {
        hideDropdown();
        return;
      }
      showDropdown(menuBtn, (menu) => {
        const rename = document.createElement("button");
        rename.type = "button";
        rename.textContent = "Rename";
        rename.addEventListener("click", () => renameFile(path));
        const del = document.createElement("button");
        del.type = "button";
        del.textContent = "Delete";
        del.className = "danger";
        del.addEventListener("click", () => deleteFile(path));
        menu.append(rename, del);
      });
    });
    previewHeader.append(filename, spacer, menuBtn);

    const content = document.createElement("div");
    content.className = "preview-error";
    if (data.error) {
      content.textContent = data.error;
    } else if (data.is_image) {
      const img = document.createElement("img");
      img.src = "/filecontent?path=" + encodeURIComponent(path) + "&raw=true";
      content.appendChild(img);
    } else if (data.is_binary) {
      content.className = "preview-binary";
      content.textContent = "Binary file (" + data.size + " bytes)";
    } else {
      const pre = document.createElement("pre");
      pre.textContent = data.content;
      content.appendChild(pre);
    }
    previewContent.appendChild(content);
  } catch (e) {
    setSubtitle("failed to load preview");
  }
}

function resetPreview() {
  selectedFile = null;
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

function showDropdown(anchor, buildFn) {
  hideDropdown();
  menuAnchor = anchor;
  dropdownEl.textContent = "";
  const menu = document.createElement("div");
  menu.className = "dropdown-menu";
  buildFn(menu);
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

async function boot() {
  document.addEventListener("keydown", (e) => {
    const tag = document.activeElement && document.activeElement.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      const order = ["tree", "list", "preview"];
      const idx = order.indexOf(activePane);
      const next = (idx + (e.key === "ArrowRight" ? 1 : -1) + order.length) % order.length;
      setActivePane(order[next]);
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const step = e.key === "ArrowDown" ? 1 : -1;
    if (activePane === "tree") {
      if (!treeEntries.length) return;
      e.preventDefault();
      let i = treeIndex < 0 ? (step === 1 ? 0 : treeEntries.length - 1) : treeIndex + step;
      if (i >= treeEntries.length) i = treeEntries.length - 1;
      if (i < 0) i = 0;
      focusTreeEntry(i);
      return;
    }
    if (activePane === "list") {
      if (!listEntries.length) return;
      e.preventDefault();
      let i = listIndex < 0 ? (step === 1 ? 0 : listEntries.length - 1) : listIndex + step;
      if (i >= listEntries.length) i = listEntries.length - 1;
      if (i < 0) i = 0;
      focusListEntry(i);
    }
  });

  setActivePane("list");

  const root = await fetchInfo();
  if (root) {
    await loadDir(root);
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