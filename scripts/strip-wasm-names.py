#!/usr/bin/env python3
"""WASM の `name` カスタムセクションを落として配布サイズを削る（Issue #43）。

`wasm-bindgen` はデバッグ用の関数名（`name` カスタムセクション）を残す。実行には不要で、
1.6 MB の wasm のうち 636 KB（39%）を占める。名前は文字列の繰り返しなので gzip でも
49 KB、`opt-level="z"` のビルドでは 63 KB 分ある（実測 2026-10-13）。

落とすのは `name` セクションだけで、`type` / `import` / `func` / `export` / `code` / `data`
などには触れない。壊れたビルドを配らないよう、書き換え前に構造を検証し、想定外の形なら
何も書かずに失敗する（fail-closed）。

使い方:
    python3 scripts/strip-wasm-names.py dist/wasm/shutoko_routing_bg.wasm
"""

from __future__ import annotations

import sys


def read_leb128(data: bytes, offset: int) -> tuple[int, int]:
    """符号なし LEB128 を読み、(値, 次のオフセット) を返す。"""
    value = 0
    shift = 0
    while True:
        if offset >= len(data):
            raise ValueError("unexpected end of file while reading LEB128")
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, offset
        shift += 7
        if shift > 63:
            raise ValueError("LEB128 value is too long")


def iter_sections(data: bytes):
    """(section_id, name, start, body_start, size) を順に返す。"""
    if len(data) < 8 or data[:4] != b"\x00asm":
        raise ValueError("not a wasm module (missing magic)")
    version = data[4:8]
    if version != b"\x01\x00\x00\x00":
        raise ValueError(f"unsupported wasm version {version.hex()}")
    offset = 8
    while offset < len(data):
        start = offset
        section_id = data[offset]
        offset += 1
        size, offset = read_leb128(data, offset)
        body_start = offset
        end = body_start + size
        if end > len(data):
            raise ValueError(f"section {section_id} overruns the file")
        name = None
        if section_id == 0:
            name_length, name_start = read_leb128(data, body_start)
            name = data[name_start : name_start + name_length].decode("utf-8", "replace")
            if name_start + name_length > end:
                raise ValueError("custom section name overruns its section")
        yield section_id, name, start, body_start, size
        offset = end


def strip_name_section(path: str) -> tuple[int, int]:
    """`name` カスタムセクションを除いて書き戻し、(変更前, 変更後) のバイト数を返す。"""
    with open(path, "rb") as handle:
        data = handle.read()

    removed = 0
    out = bytearray(data[:8])
    kept_other_sections = 0
    for section_id, name, start, body_start, size in iter_sections(data):
        if section_id == 0 and name == "name":
            removed += 1
            continue
        out += data[start : body_start + size]
        kept_other_sections += 1

    if removed == 0:
        raise SystemExit(f"{path}: no `name` custom section found; refusing to rewrite")
    if removed > 1:
        raise SystemExit(f"{path}: {removed} `name` sections found; refusing to rewrite")
    if kept_other_sections == 0:
        raise SystemExit(f"{path}: stripping would leave an empty module; refusing to rewrite")
    # 書き戻したものが読めることを確認してから置き換える。
    stripped = bytes(out)
    list(iter_sections(stripped))

    with open(path, "wb") as handle:
        handle.write(stripped)
    return len(data), len(stripped)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} <wasm-file>", file=sys.stderr)
        return 2
    before, after = strip_name_section(argv[1])
    print(
        f"stripped `name` section from {argv[1]}: {before} -> {after} bytes "
        f"(-{before - after}, {(before - after) * 100 // before}%)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
