let rootLabel = document.getElementById("root-label");
let filterInput = document.getElementById("file-filter");
let refreshBtn = document.getElementById("refresh-btn");
let treeEl = document.getElementById("file-tree");
let subtitleEl = document.getElementById("files-subtitle");
let previewHeader = document.getElementById("preview-header");
let previewContent = document.getElementById("preview-content");

let rootEntries = [];
let expandedDirs = new Set();
let dirChildren = new Map();
let selectedFile = null;
let searchResults = [];
let searchMode = false;

const ICON_DIR = "icons/";
const DEFAULT_FILE_ICON = "text-x-generic.svg";
const DEFAULT_BINARY_ICON = "application-octet-stream.svg";

const FOLDER_SPECIAL = {
  ".git": "folder-git",
  "node_modules": "folder-develop",
  ".venv": "folder-develop",
  "venv": "folder-develop",
  "env": "folder-develop",
  ".env": "folder-develop",
  "target": "folder-develop",
  "build": "folder-develop",
  "dist": "folder-develop",
  ".idea": "folder-develop",
  ".vscode": "folder-develop",
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
};

function iconFileForEntry(entry) {
  const base = entry.name;
  const low = base.toLowerCase();
  const nameIcon = NAME_ICONS[low] ||
    (low.startsWith("readme") ? "text-x-readme" : null) ||
    (low.startsWith("license") || low === "copying" || low.startsWith("copying") ? "text-x-copying" : null);
  if (entry.is_dir) {
    const open = expandedDirs.has(entry.path);
    const special = FOLDER_SPECIAL[low];
    return ICON_DIR + ((open || !special) ? (open ? "folder-open" : "folder") : special) + ".svg";
  }
  const dot = low.lastIndexOf(".");
  let ext = dot < 0 ? "" : low.slice(dot + 1);
  if (dot === 0 && ext.length > 1) ext = low.slice(1);
  const icon = nameIcon || EXT_ICONS[ext] || DEFAULT_FILE_ICON;
  return ICON_DIR + icon + ".svg";
}

function iconEl(entry) {
  const img = document.createElement("img");
  img.className = "tree-icon";
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
    rootLabel.textContent = info.root;
  } catch {}
}

async function fetchFileTree() {
  if (searchMode) return;
  try {
    const res = await fetch("/filetree?path=");
    rootEntries = await res.json();
    renderTree();
  } catch (e) {
    setSubtitle("failed to list root");
  }
}

async function fetchDirChildren(dirPath) {
  const key = dirPath;
  if (dirChildren.has(key)) return dirChildren.get(key);
  try {
    const res = await fetch("/filetree?path=" + encodeURIComponent(dirPath));
    const entries = await res.json();
    dirChildren.set(key, entries);
    return entries;
  } catch {
    return [];
  }
}

async function fetchSearch(query) {
  const res = await fetch("/filesearch?q=" + encodeURIComponent(query));
  searchResults = await res.json();
  searchMode = true;
  renderTree();
}

function renderTree() {
  treeEl.textContent = "";
  if (searchMode) {
    setSubtitle(searchResults.length ? searchResults.length + " matches" : "No matches");
    for (const entry of searchResults) {
      const row = document.createElement("div");
      row.className = "tree-item" + (selectedFile === entry.path ? " selected" : "");
      const icon = iconEl(entry);
      const name = document.createElement("span");
      name.textContent = entry.path;
      name.title = entry.path;
      row.append(icon, name);
      row.addEventListener("click", () => selectEntry(entry));
      treeEl.appendChild(row);
    }
    return;
  }

  const rowsFlat = [];
  (function flatten(list, depth) {
    const sorted = [...list].sort((a, b) =>
      (b.is_dir - a.is_dir) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
    );
    for (const entry of sorted) {
      rowsFlat.push([entry, depth]);
      if (entry.is_dir && expandedDirs.has(entry.path) && dirChildren.has(entry.path)) {
        flatten(dirChildren.get(entry.path), depth + 1);
      }
    }
  })(rootEntries, 0);
  for (const [entry, depth] of rowsFlat) {
    const row = document.createElement("div");
    row.className = "tree-item" + (selectedFile === entry.path ? " selected" : "");
    row.style.paddingLeft = 8 + depth * 16 + "px";
    const icon = iconEl(entry);
    const name = document.createElement("span");
    name.textContent = entry.name;
    name.title = entry.path;
    row.append(icon, name);
    row.addEventListener("click", () => selectEntry(entry));
    treeEl.appendChild(row);
  }
}

async function selectEntry(entry) {
  selectedFile = entry.path;
  if (entry.is_dir) {
    await toggleDir(entry);
    return;
  }
  renderTree();
  await selectFile(entry.path);
}

async function toggleDir(entry) {
  if (expandedDirs.has(entry.path)) {
    expandedDirs.delete(entry.path);
    for (const key of [...dirChildren.keys()]) {
      if (key.startsWith(entry.path + "/")) dirChildren.delete(key);
    }
  } else {
    expandedDirs.add(entry.path);
    const children = await fetchDirChildren(entry.path);
    selectedFile = null;
    renderTree();
    if (children.length === 0) setSubtitle("empty");
  }
  renderTree();
}

async function selectFile(path) {
  renderTree();
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
  previewHeader.textContent = "";
  previewContent.textContent = "";
  const p = document.createElement("p");
  p.className = "muted";
  p.textContent = "Select a file to preview";
  previewContent.appendChild(p);
}

filterInput.addEventListener("input", () => {
  const q = filterInput.value.trim();
  if (!q) {
    searchMode = false;
    renderTree();
    return;
  }
  fetchSearch(q);
});

refreshBtn.addEventListener("click", () => {
  dirChildren.clear();
  fetchFileTree();
  if (selectedFile) selectFile(selectedFile);
});

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
  const base = path.slice(slash + 1);
  const dir = slash < 0 ? "" : path.slice(0, slash);
  const target = prompt("Rename \"" + base + "\" to:", base);
  if (!target || target === base || target.includes("/")) return;
  const newPath = dir ? dir + "/" + target : target;
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
  dirChildren.clear();
  await Promise.all([...expandedDirs].map((d) => fetchDirChildren(d)));
  await fetchFileTree();
  if (selectedFile &&
      (selectedFile === path ||
        selectedFile.startsWith(path + "/") ||
        path.startsWith(selectedFile + "/"))) {
    selectFile(selectedFile);
  }
}

function connectSocket() {
  const proto = location.protocol === "https:" ? "wss://" : "ws://";
  const ws = new WebSocket(proto + location.host + "/ws");
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

fetchInfo();
fetchFileTree();
connectSocket();