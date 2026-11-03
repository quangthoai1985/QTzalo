"""Read-only map of an Electron app.asar: file tree summary, native modules, and which files
contain given strings. Never executes anything from the archive.

usage: python -I asar_map.py <app.asar> <out.txt> term1 term2 ...
"""
import json
import os
import struct
import sys


def read_header(f):
    f.seek(0)
    _, header_size = struct.unpack("<II", f.read(8))
    _, json_len = struct.unpack("<II", f.read(8))
    header = json.loads(f.read(json_len).decode("utf-8"))
    return header, 8 + header_size


def walk(node, prefix=""):
    for name, entry in node.get("files", {}).items():
        path = f"{prefix}/{name}" if prefix else name
        if "files" in entry:
            yield from walk(entry, path)
        elif "link" not in entry:
            yield path, entry


def main():
    asar, out_path, terms = sys.argv[1], sys.argv[2], sys.argv[3:]
    unpacked_dir = asar + ".unpacked"
    with open(asar, "rb") as f:
        header, base = read_header(f)
        files = list(walk(header))
        lines = [f"files: {len(files)}"]
        tops = {}
        for path, e in files:
            top = path.split("/")[0]
            n, s = tops.get(top, (0, 0))
            tops[top] = (n + 1, s + int(e.get("size", 0)))
        lines.append("top-level (files, bytes):")
        for top, (n, s) in sorted(tops.items(), key=lambda kv: -kv[1][1])[:25]:
            lines.append(f"  {top}: {n}, {s}")
        lines.append("native modules (*.node):")
        for path, e in files:
            if path.endswith(".node"):
                lines.append(f"  {path} size={e.get('size')} unpacked={bool(e.get('unpacked'))}")
        lines.append("largest .js files:")
        js = sorted((p for p in files if p[0].endswith(".js")), key=lambda p: -int(p[1].get("size", 0)))
        for path, e in js[:15]:
            lines.append(f"  {path} {e.get('size')}")
        hits = {t: [] for t in terms}
        for path, e in files:
            if not path.endswith((".js", ".json", ".html", ".mjs", ".cjs", ".node", ".wasm")):
                continue
            size = int(e.get("size", 0))
            if e.get("unpacked"):
                p = os.path.join(unpacked_dir, *path.split("/"))
                try:
                    with open(p, "rb") as g:
                        data = g.read()
                except OSError:
                    continue
            else:
                f.seek(base + int(e["offset"]))
                data = f.read(size)
            for t in terms:
                c = data.count(t.encode("utf-8"))
                if c:
                    hits[t].append((path, c))
        lines.append("string hits (file: count):")
        for t in terms:
            lines.append(f"  {t}:")
            for path, c in hits[t][:8]:
                lines.append(f"    {path}: {c}")
            if len(hits[t]) > 8:
                lines.append(f"    … +{len(hits[t]) - 8} files")
    with open(out_path, "w", encoding="utf-8") as o:
        o.write("\n".join(lines) + "\n")
    print(f"wrote {out_path} ({len(lines)} lines)")


if __name__ == "__main__":
    main()
