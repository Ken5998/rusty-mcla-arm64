#!/usr/bin/env python3
"""Sets keys of Rusty MCLA's settings.toml, keeping every other line as it is
(the game writes the file too: Alt+Enter, the options menu).

    scripts/configure.py FILE section.key=value [section.key=value ...]

A value is written as given when it is a number or true/false, else quoted.
Needs only Python's standard library.
"""
import re
import sys
from pathlib import Path


def toml_value(text: str) -> str:
    if text in ("true", "false") or re.fullmatch(r"-?\d+(\.\d+)?", text):
        return text
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def set_keys(lines: list[str], wanted: dict[tuple[str, str], str]) -> list[str]:
    out: list[str] = []
    section = ""
    done: set[tuple[str, str]] = set()

    def missing_for(name: str) -> list[str]:
        return [f"{k} = {v}" for (s, k), v in wanted.items() if s == name and (s, k) not in done]

    for line in lines:
        header = re.fullmatch(r"\s*\[([A-Za-z0-9_.-]+)\]\s*(#.*)?", line)
        if header:
            # (the section's new keys go before the blank lines that end it)
            blanks = 0
            while out and not out[-1].strip():
                out.pop()
                blanks += 1
            for extra in missing_for(section):
                out.append(extra)
                done.add((section, extra.split(" = ")[0]))
            out.extend([""] * blanks)
            section = header.group(1)
            out.append(line)
            continue
        key = re.match(r"\s*([A-Za-z0-9_-]+)\s*=", line)
        if key and (section, key.group(1)) in wanted:
            out.append(f"{key.group(1)} = {wanted[(section, key.group(1))]}")
            done.add((section, key.group(1)))
            continue
        out.append(line)
    for extra in missing_for(section):
        out.append(extra)
        done.add((section, extra.split(" = ")[0]))
    for name in dict.fromkeys(s for s, _ in wanted):
        rest = missing_for(name)
        if rest:
            if out and out[-1].strip():
                out.append("")
            out.append(f"[{name}]")
            out.extend(rest)
            done.update((name, r.split(" = ")[0]) for r in rest)
    return out


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        sys.exit(__doc__)
    path = Path(argv[1])
    wanted: dict[tuple[str, str], str] = {}
    for pair in argv[2:]:
        name, _, value = pair.partition("=")
        section, _, key = name.rpartition(".")
        if not section or not key or not _:
            sys.exit(f"error: {pair} is not section.key=value")
        wanted[(section, key)] = toml_value(value)
    lines = path.read_text(encoding="utf-8").splitlines() if path.is_file() else []
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text("\n".join(set_keys(lines, wanted)) + "\n", encoding="utf-8")
    tmp.replace(path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
