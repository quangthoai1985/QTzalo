"""Print short windows around strings inside files of an Electron app.asar (read-only).

usage: python -I asar_snip.py <app.asar> <out.txt> <radius> <file-in-asar>=<term>[,<term>...] ...
A file entry ending in "!" is dumped whole (only for small files).
"""
import json
import os
import struct
import sys


def read_header(f):
    f.seek(0)
    _, header_size = struct.unpack("<II", f.read(8))
    _, json_len = struct.unpack("<II", f.read(8))
    return json.loads(f.read(json_len).decode("utf-8")), 8 + header_size


def find(node, path):
    for part in path.split("/"):
        node = node["files"][part]
    return node


def main():
    asar, out_path, radius = sys.argv[1], sys.argv[2], int(sys.argv[3])
    out = []
    with open(asar, "rb") as f:
        header, base = read_header(f)
        for spec in sys.argv[4:]:
            path, _, terms = spec.partition("=")
            whole = path.endswith("!")
            path = path.rstrip("!")
            e = find(header, path)
            if e.get("unpacked"):
                with open(os.path.join(asar + ".unpacked", *path.split("/")), "rb") as g:
                    data = g.read()
            else:
                f.seek(base + int(e["offset"]))
                data = f.read(int(e["size"]))
            text = data.decode("utf-8", "replace")
            if whole:
                out.append(f"==== {path} (whole, {len(text)} chars)\n{text}")
                continue
            for term in filter(None, terms.split(",")):
                i, n = 0, 0
                while True:
                    i = text.find(term, i)
                    if i < 0 or n >= 6:
                        break
                    out.append(f"==== {path} @{i} [{term}]\n{text[max(0, i - radius):i + radius]}")
                    i += len(term)
                    n += 1
    with open(out_path, "w", encoding="utf-8") as o:
        o.write("\n\n".join(out) + "\n")
    print(f"wrote {out_path} ({len(out)} snippets)")


if __name__ == "__main__":
    main()
